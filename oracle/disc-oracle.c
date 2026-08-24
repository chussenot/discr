/* disc-oracle -- run Disc (Loriciel 1990) under Musashi as a deterministic,
 * headless trace generator.  Hatari stays the reference; this exists to make
 * traces cheap enough for a test suite.
 *
 * The machine it emulates is exactly what reports/oracle-scope.md measured the
 * game to need, and nothing else: 1 MB of RAM, a level-4 VBL at $8198, an IKBD
 * ACIA, and write-only stubs for the PSG, palette, screen base and the four
 * MFP timer registers.  Any other hardware access aborts the run, because an
 * oracle that quietly returns 0 for a register nobody stubbed is worse than no
 * oracle at all.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <openssl/sha.h>
#include "musashi/m68k.h"

#define RAM_SIZE   0x100000u
#define ADDR_MASK  0x00ffffffu
#define VBL_HANDLER 0x8198u
#define STATE_LO   0x0000u
#define STATE_HI   0x8000u
/* PAL: 512 cycles/line * 313 lines at 8 MHz */
#define FRAME_CYCLES_DEFAULT 160256

/* MFP vector base is $100.  ACIA is source 6 -> vector $46 ($118);
 * Timer A is source 13 -> vector $4d ($134). Higher source number wins. */
#define ACIA_VECTOR   0x46
#define TIMERA_VECTOR 0x4d
/* ST clocks: MFP 2.4576 MHz, CPU 8.021247 MHz (PAL) */
#define MFP_HZ 2457600.0
#define CPU_HZ 8021247.0

static uint8_t ram[RAM_SIZE];
static int permissive = 0, debug_regs = 0;
static long dump_pcs = 0;
static long win_lo = -1, win_hi = -1;
static long frame_cycles = FRAME_CYCLES_DEFAULT;
static double cycles_now = 0;
static long unstubbed = 0;

static uint8_t mfp_tacr, mfp_tbcr, mfp_tadr, mfp_tbdr;

/* ---- interrupt state --------------------------------------------------- */
static int vbl_pending = 0, acia_pending = 0, tima_pending = 0;

static void refresh_irq(void)
{
    m68k_set_irq((tima_pending || acia_pending) ? 6 : (vbl_pending ? 4 : 0));
}

int disc_int_ack(int level)
{
    if (level == 6) {
        /* MFP priority is by source number: Timer A (13) outranks ACIA (6). */
        if (tima_pending) { tima_pending = 0; refresh_irq(); return TIMERA_VECTOR; }
        /* The ACIA does NOT deassert on acknowledge -- it keeps its interrupt
         * up for as long as a byte is waiting in the receive register, and the
         * handler clears it by reading $FFFC02.  Clearing it here instead lost
         * the second byte of every IKBD packet: the decoder is a two-interrupt
         * state machine ($FF, then the joystick state), so $6c58 never
         * updated.  An idle script cannot see this; the input differ found it
         * on the first press. */
        return ACIA_VECTOR;
    }
    if (level == 4) { vbl_pending = 0; refresh_irq(); return M68K_INT_ACK_AUTOVECTOR; }
    return M68K_INT_ACK_SPURIOUS;
}

/* ---- MFP Timer A -------------------------------------------------------
 * Phase 0 said both timers were RAM-free below $8000 and could be skipped.
 * That was right about Timer A's steady-state path and wrong about its exit
 * path: when the sample stream hits its terminating 0, $83fe clears $6c5b and
 * $6c5c -- the "sound effect busy" latch that the disc engine sets at $a6c4
 * before pointing USP at the sample and starting the timer.  The differ found
 * it as a 2-byte disagreement.  So Timer A is emulated for real. */
static const int MFP_PRESCALE[8] = {0, 4, 10, 16, 50, 64, 100, 200};
static double tima_deadline = 0;
static double tima_period = 0;

static void tima_reload(void)
{
    int ps = MFP_PRESCALE[mfp_tacr & 7];
    int cnt = mfp_tadr ? mfp_tadr : 256;
    if (!ps) { tima_period = 0; return; }         /* stopped */
    tima_period = (double)ps * cnt * CPU_HZ / MFP_HZ;
    tima_deadline = cycles_now + tima_period;
}

/* ---- frame boundary detection -------------------------------------------
 * The sampling point is "PC == $8198, before that instruction runs", and it
 * cannot be seen from outside m68k_execute(): the interrupt dispatch and the
 * handler's first instruction happen inside a single call, so an external
 * `if (PC == 0x8198)` loop always observes $819c and samples a frame late.
 * (Measured as $6ab4 advancing by 2 per frame.)  Musashi's instruction hook
 * DOES fire before each instruction, so the frame is emitted from in there.
 * m68k_end_timeslice() then hands control back -- it does not stop the
 * current instruction, but by that point we have already sampled. */
static int sample_armed = 0, sampled = 0;
static long cur_frame = 0;
static FILE *trace_out = NULL;
static void emit_frame(FILE *out, long frame);

void disc_instr_hook(unsigned int pc)
{
    if (sample_armed && (pc & ADDR_MASK) == VBL_HANDLER) {
        emit_frame(trace_out, cur_frame);
        sample_armed = 0;
        sampled = 1;
        m68k_end_timeslice();
    }
}

/* ---- IKBD ACIA --------------------------------------------------------- */
/* The real IKBD only emits a joystick packet when the state CHANGES (measured:
 * holding a direction produced 2 ACIA interrupts, not one per frame), so the
 * queue is fed on transitions only. */
static uint8_t ikbd_q[256];
static int ikbd_head = 0, ikbd_tail = 0;

/* Packets do not arrive on the frame boundary.  The IKBD runs at 7812.5 baud
 * and is not synchronised to the VBL, so a byte lands somewhere inside the
 * frame.  MEASURED (see reports/exploration-report.md):
 *
 *   - ACIA handler entry ($8370) is UNIFORM over the frame: 24 samples spread
 *     evenly from FrameCycles 15960 to 153184.
 *   - The game consumes $6c58 at ~23200 cycles (scanline ~45), found by
 *     bisecting --ikbd-delay against the Hatari reference: agreement steps
 *     from 61 frames to 364 between 23125 and 23312.
 *
 * So any delay above ~23.3k reproduces the common case, and the plateau runs
 * to at least 159000.  Half a frame sits in the middle of it.  The residual
 * is real and unmodellable determinstically: ~14.5% of true arrivals land
 * before consumption and are acted on in the SAME frame, a per-packet coin
 * flip.  A fixed delay trades that for reproducibility on purpose. */
#define IKBD_DELAY_DEFAULT 80128        /* half a PAL frame, inside the plateau */
static uint8_t stage_q[64];
static int stage_n = 0;
static double stage_at = -1;
static long ikbd_delay = -1;          /* cycles into the frame; <0 = half */

static void ikbd_push(uint8_t b)
{
    int n = (ikbd_tail + 1) & 255;
    if (n == ikbd_head) { fprintf(stderr, "ikbd queue overflow\n"); exit(3); }
    ikbd_q[ikbd_tail] = b;
    ikbd_tail = n;
    acia_pending = 1;          /* RDRF: a byte is waiting */
    refresh_irq();
}

static int ikbd_empty(void) { return ikbd_head == ikbd_tail; }

static uint8_t ikbd_pop(void)
{
    uint8_t b;
    if (ikbd_empty()) return 0;
    b = ikbd_q[ikbd_head];
    ikbd_head = (ikbd_head + 1) & 255;
    acia_pending = !ikbd_empty();   /* still set while more bytes wait */
    refresh_irq();
    return b;
}

/* ---- hardware stubs ---------------------------------------------------- */

static void io_unstubbed(const char *op, unsigned int addr)
{
    unstubbed++;
    fprintf(stderr, "UNSTUBBED %s $%06x at PC $%06x\n",
            op, addr, m68k_get_reg(NULL, M68K_REG_PPC));
    if (!permissive) {
        fprintf(stderr,
            "aborting: this address is not in the Phase-0 stub list.  Either\n"
            "the game reached code the scope report never exercised, or the\n"
            "list is wrong.  Re-run Phase 0 rather than adding a silent 0.\n");
        exit(4);
    }
}

static unsigned int io_read(unsigned int addr, int size)
{
    switch (addr) {
    case 0xfffc00:                       /* IKBD ACIA control/status */
        return (ikbd_empty() ? 0x02 : 0x03) | (ikbd_empty() ? 0 : 0x80);
    case 0xfffc02:                       /* IKBD ACIA data */
        return ikbd_pop();
    case 0xfffa19: return mfp_tacr;
    case 0xfffa1b: return mfp_tbcr;
    case 0xfffa1f: return mfp_tadr;
    case 0xfffa21: return mfp_tbdr;
    default:
        io_unstubbed(size == 1 ? "read.b" : (size == 2 ? "read.w" : "read.l"), addr);
        return 0;
    }
}

static void io_write(unsigned int addr, unsigned int val, int size)
{
    /* PSG (sound), palette, screen base: write-only, no effect on game state.
     * Timer A/B registers must read back what was written. */
    if (addr >= 0xff8800 && addr <= 0xff8807) return;          /* PSG + mirror */
    if (addr >= 0xff8240 && addr <= 0xff825f) return;          /* palette */
    if (addr == 0xff8201 || addr == 0xff8203) return;          /* screen base */
    switch (addr) {
    case 0xfffa19: {
        int was = mfp_tacr & 7;
        mfp_tacr = val;
        if ((val & 7) && !was) tima_reload();      /* start */
        else if (!(val & 7)) tima_period = 0;      /* stop */
        return;
    }
    case 0xfffa1b: mfp_tbcr = val; return;
    case 0xfffa1f: mfp_tadr = val; return;
    case 0xfffa21: mfp_tbdr = val; return;
    default:
        io_unstubbed(size == 1 ? "write.b" : (size == 2 ? "write.w" : "write.l"), addr);
        (void)val;
    }
}

/* ---- memory callbacks --------------------------------------------------
 * RAM is kept as raw big-endian bytes -- the exact layout Hatari's savebin
 * produces -- so a hash of the buffer is comparable to Hatari's by
 * construction, and the byte assembly happens here instead. */
static inline int is_io(unsigned int a) { return a >= 0xff8000u; }

unsigned int m68k_read_memory_8(unsigned int address)
{
    unsigned int a = address & ADDR_MASK;
    if (a < RAM_SIZE) return ram[a];
    if (is_io(a)) return io_read(a, 1);
    io_unstubbed("read.b", a);
    return 0;
}

unsigned int m68k_read_memory_16(unsigned int address)
{
    unsigned int a = address & ADDR_MASK;
    if (a + 1 < RAM_SIZE) return ((unsigned)ram[a] << 8) | ram[a + 1];
    if (is_io(a)) return io_read(a, 2);
    io_unstubbed("read.w", a);
    return 0;
}

unsigned int m68k_read_memory_32(unsigned int address)
{
    unsigned int a = address & ADDR_MASK;
    if (a + 3 < RAM_SIZE)
        return ((unsigned)ram[a] << 24) | ((unsigned)ram[a + 1] << 16) |
               ((unsigned)ram[a + 2] << 8) | ram[a + 3];
    if (is_io(a)) return (io_read(a, 2) << 16) | io_read(a + 2, 2);
    io_unstubbed("read.l", a);
    return 0;
}

void m68k_write_memory_8(unsigned int address, unsigned int value)
{
    unsigned int a = address & ADDR_MASK;
    if (a < RAM_SIZE) { ram[a] = value & 0xff; return; }
    if (is_io(a)) { io_write(a, value & 0xff, 1); return; }
    io_unstubbed("write.b", a);
}

void m68k_write_memory_16(unsigned int address, unsigned int value)
{
    unsigned int a = address & ADDR_MASK;
    if (a + 1 < RAM_SIZE) { ram[a] = (value >> 8) & 0xff; ram[a + 1] = value & 0xff; return; }
    if (is_io(a)) { io_write(a, value & 0xffff, 2); return; }
    io_unstubbed("write.w", a);
}

void m68k_write_memory_32(unsigned int address, unsigned int value)
{
    unsigned int a = address & ADDR_MASK;
    if (a + 3 < RAM_SIZE) {
        ram[a] = (value >> 24) & 0xff; ram[a + 1] = (value >> 16) & 0xff;
        ram[a + 2] = (value >> 8) & 0xff; ram[a + 3] = value & 0xff;
        return;
    }
    if (is_io(a)) { io_write(a, value >> 16, 2); io_write(a + 2, value & 0xffff, 2); return; }
    io_unstubbed("write.l", a);
}

/* Musashi asks for these when disassembling / for immediate reads. */
unsigned int m68k_read_disassembler_16(unsigned int a) { return m68k_read_memory_16(a); }
unsigned int m68k_read_disassembler_32(unsigned int a) { return m68k_read_memory_32(a); }

/* Run n CPU cycles, breaking the run so Timer A fires on schedule. */
static void run_cycles(double n)
{
    while (n > 0) {
        int chunk = (int)(n > 100000 ? 100000 : n), got;
        if (stage_at >= 0) {
            double d = stage_at - cycles_now;
            if (d < 1) d = 1;
            if (d < chunk) chunk = (int)d;
        }
        if (tima_period > 0) {
            double d = tima_deadline - cycles_now;
            if (d < 1) d = 1;
            if (d < chunk) chunk = (int)d;
        }
        if (chunk < 1) chunk = 1;
        got = m68k_execute(chunk);
        cycles_now += got;
        n -= got;
        if (stage_at >= 0 && cycles_now >= stage_at) {
            int k;
            for (k = 0; k < stage_n; k++) ikbd_push(stage_q[k]);
            stage_n = 0; stage_at = -1;
        }
        if (tima_period > 0 && cycles_now >= tima_deadline) {
            tima_pending = 1;
            refresh_irq();
            tima_deadline += tima_period;
            if (tima_deadline < cycles_now) tima_deadline = cycles_now + tima_period;
        }
    }
}

/* ---- seed loading ------------------------------------------------------ */
/* A deliberately small scanner over our own JSON: find "KEY" then the next
 * integer.  Not a general parser -- the file is written by collect.py. */
static long json_int(const char *buf, const char *key, int *found)
{
    char pat[64];
    const char *p;
    snprintf(pat, sizeof pat, "\"%s\"", key);
    p = strstr(buf, pat);
    *found = 0;
    if (!p) return 0;
    p += strlen(pat);
    while (*p && *p != ':') p++;
    if (!*p) return 0;
    p++;
    while (*p == ' ' || *p == '\t' || *p == '\n') p++;
    *found = 1;
    return strtol(p, NULL, 0);
}

static char *slurp(const char *path, long *len)
{
    FILE *f = fopen(path, "rb");
    char *b;
    if (!f) { perror(path); exit(2); }
    fseek(f, 0, SEEK_END); *len = ftell(f); fseek(f, 0, SEEK_SET);
    b = malloc(*len + 1);
    if (fread(b, 1, *len, f) != (size_t)*len) { perror(path); exit(2); }
    b[*len] = 0;
    fclose(f);
    return b;
}

static void hexdigest(const uint8_t *p, size_t n, char out[65])
{
    unsigned char d[SHA256_DIGEST_LENGTH];
    int i;
    SHA256(p, n, d);
    for (i = 0; i < SHA256_DIGEST_LENGTH; i++) sprintf(out + i * 2, "%02x", d[i]);
    out[64] = 0;
}

/* ---- input script ------------------------------------------------------
 *   j <frame> <joy1hex> <joy0hex>     joystick state at the start of <frame>
 *   k <frame> <scancode> <0|1>        key break(0)/make(1)
 * Lines starting with '#' are comments. */
struct ev { long frame; int kind, a, b; };
static struct ev *evs; static int nev;

static void load_script(const char *path)
{
    long len; char *buf, *line, *save;
    int cap = 64;
    if (!path) return;
    buf = slurp(path, &len);
    evs = malloc(cap * sizeof *evs);
    for (line = strtok_r(buf, "\n", &save); line; line = strtok_r(NULL, "\n", &save)) {
        long fr; int x, y; char k;
        while (*line == ' ') line++;
        if (*line == '#' || !*line) continue;
        if (sscanf(line, "%c %ld %x %x", &k, &fr, &x, &y) != 4) {
            fprintf(stderr, "bad script line: %s\n", line); exit(2);
        }
        if (nev == cap) { cap *= 2; evs = realloc(evs, cap * sizeof *evs); }
        evs[nev].frame = fr; evs[nev].kind = k; evs[nev].a = x; evs[nev].b = y;
        nev++;
    }
    free(buf);
}

static int joy1_state = 0, joy0_state = 0;

/* ---- autopilot ----------------------------------------------------------
 * The oracle is deterministic and runs at ~2500 fps, so it can close the loop
 * on itself: read the game's own state each frame and synthesize the IKBD
 * packets that steer the player, instead of guessing a script blind.
 *
 * The servo needs no knowledge of the disc's coordinate space.  $6cb0 is the
 * player's grid cell (8 + column + 4 if far row, verified in Part 5), so
 * "walk to cell N" is just: cell too high -> Left, too low -> Right.  Whatever
 * it finds is written out as a plain script, because the differ has to replay
 * the exact sequence through Hatari, which cannot run an autopilot. */
static int ap_on = 0, ap_cell = 0, ap_fire_period = 0, ap_fire_from = 0;
static FILE *ap_script = NULL;
static int ap_last_joy = -1;

static void stage_push(uint8_t b)
{
    if (stage_n < (int)sizeof stage_q) stage_q[stage_n++] = b;
}

/* Decide this frame's joystick state from the game's own memory. */
static int autopilot_joy(long frame)
{
    int cell = (ram[0x6cb0] << 8) | ram[0x6cb1];
    int joy = 0;
    if (ap_cell) {
        if (cell > ap_cell) joy = 0x04;          /* Left  */
        else if (cell < ap_cell) joy = 0x08;     /* Right */
    }
    if (ap_fire_period && frame >= ap_fire_from &&
        ((frame - ap_fire_from) % ap_fire_period) < 2)
        joy |= 0x80;
    return joy;
}

static void apply_events(long frame)
{
    int i;
    stage_n = 0;
    if (ap_on) {
        int joy = autopilot_joy(frame);
        if (joy != joy1_state) {
            joy1_state = joy;
            stage_push(0xff); stage_push(joy);
            if (ap_script) fprintf(ap_script, "j %ld %02x 00\n", frame, joy);
            ap_last_joy = joy;
        }
        stage_at = stage_n ? cycles_now + (ikbd_delay >= 0 ? ikbd_delay
                                           : IKBD_DELAY_DEFAULT) : -1;
        return;
    }
    for (i = 0; i < nev; i++) {
        if (evs[i].frame != frame) continue;
        if (evs[i].kind == 'j') {
            if (evs[i].a != joy1_state) { joy1_state = evs[i].a; stage_push(0xff); stage_push(joy1_state); }
            if (evs[i].b != joy0_state) { joy0_state = evs[i].b; stage_push(0xfe); stage_push(joy0_state); }
        } else if (evs[i].kind == 'k') {
            stage_push(evs[i].b ? (evs[i].a & 0x7f) : (evs[i].a | 0x80));
        }
    }
    stage_at = stage_n ? cycles_now + (ikbd_delay >= 0 ? ikbd_delay
                                       : IKBD_DELAY_DEFAULT) : -1;
}

/* ---- state emission ---------------------------------------------------- */
static unsigned rd16(unsigned a) { return ((unsigned)ram[a] << 8) | ram[a + 1]; }

static void emit_frame(FILE *out, long frame)
{
    char h[65];
    int i;
    hexdigest(ram + STATE_LO, STATE_HI - STATE_LO, h);
    /* $6c59 is player 2's joystick byte; $6da1 is the byte the one-player AI
     * ($d2cc) synthesises in its place, and $6da0 selects between them
     * ($10eac).  $6d9a is the active bonus code.  Part 10. */
    fprintf(out, "{\"frame\":%ld,\"vbl_6ab4\":%u,\"joy_6c58\":%u"
                 ",\"joy_6c59\":%u,\"ai_6da1\":%u,\"mode_6da0\":%u,\"bonus_6d9a\":%d",
            frame, rd16(0x6ab4), ram[0x6c58],
            ram[0x6c59], ram[0x6da1], ram[0x6da0], (int16_t)rd16(0x6d9a));
    fprintf(out, ",\"player\":[");
    for (i = 0; i < 2; i++) {
        unsigned b = 0x6ca0 + i * 0x80;
        /* Part 10b: +$3a is the animation sequence cursor ($6cda / $6d5a),
         * which the serve gates on ($c06e compares it with $4602); +$6e is the
         * player's throw dir_kind, copied into disc+$0a; +$70 its magnitude,
         * copied into disc+$16 at $a9cc. */
        fprintf(out, "%s{\"x\":%u,\"y\":%u,\"facing\":%u,\"state\":%u,\"cell\":%u"
                     ",\"anim\":%u,\"throw_dk\":%d,\"throw_mag\":%d}",
                i ? "," : "", rd16(b + 2), rd16(b + 6), ram[b + 9], ram[b + 0x0e], rd16(b + 0x10),
                (rd16(b + 0x3a) << 16) | rd16(b + 0x3c),
                (int16_t)rd16(b + 0x6e), (int16_t)rd16(b + 0x70));
    }
    fprintf(out, "],\"disc\":[");
    for (i = 0; i < 8; i++) {
        unsigned b = 0x6e3e + i * 0x42;
        /* "flag" is the UNSIGNED +$0a and predates Part 10; "dk" is the same
         * word read signed, which is what it is.  +$08/$10/$11/$12/$16 were
         * added once $a4ea was disassembled: vel_y is integrated at $a556,
         * +$10 is the active byte tested at $a4f0, +$11 the owner tested at
         * $a55e, +$12 the per-disc hook called at $a54c and cleared on every
         * bounce, +$16 the damage subtracted at $a31c. */
        fprintf(out, "%s{\"wx\":%d,\"wy\":%d,\"wz\":%d,\"vx\":%d,\"vy\":%d,\"flag\":%u"
                     ",\"dk\":%d,\"act\":%u,\"own\":%u,\"hook\":%u,\"dmg\":%d"
                     ",\"sx\":%d,\"sy\":%d}",
                i ? "," : "", (int16_t)rd16(b), (int16_t)rd16(b + 2), (int16_t)rd16(b + 4),
                (int16_t)rd16(b + 6), (int16_t)rd16(b + 8), rd16(b + 0x0a),
                (int16_t)rd16(b + 0x0a), ram[b + 0x10], ram[b + 0x11],
                (rd16(b + 0x12) << 16) | rd16(b + 0x14), (int16_t)rd16(b + 0x16),
                (int16_t)rd16(b + 0x0c), (int16_t)rd16(b + 0x0e));
    }
    fprintf(out, "],\"grid\":[");
    for (i = 0; i < 17; i++) {
        unsigned b = 0x7616 + i * 8;
        fprintf(out, "%s[%u,%u]", i ? "," : "", rd16(b), rd16(b + 2));
    }
    fprintf(out, "],\"state_sha256\":\"%s\"", h);
    if (win_lo >= 0) {
        long a;
        fprintf(out, ",\"win_lo\":%ld,\"mem\":\"", win_lo);
        for (a = win_lo; a < win_hi && a < (long)RAM_SIZE; a++)
            fprintf(out, "%02x", ram[a]);
        fprintf(out, "\"");
    }
    if (debug_regs)
        fprintf(out, ",\"pc\":%u,\"sr\":%u,\"usp\":%u",
                m68k_get_reg(NULL, M68K_REG_PC), m68k_get_reg(NULL, M68K_REG_SR),
                m68k_get_reg(NULL, M68K_REG_USP));
    fprintf(out, "}\n");
}

/* ---- main -------------------------------------------------------------- */
static const struct { const char *n; m68k_register_t r; } REGS[] = {
    {"D0",M68K_REG_D0},{"D1",M68K_REG_D1},{"D2",M68K_REG_D2},{"D3",M68K_REG_D3},
    {"D4",M68K_REG_D4},{"D5",M68K_REG_D5},{"D6",M68K_REG_D6},{"D7",M68K_REG_D7},
    {"A0",M68K_REG_A0},{"A1",M68K_REG_A1},{"A2",M68K_REG_A2},{"A3",M68K_REG_A3},
    {"A4",M68K_REG_A4},{"A5",M68K_REG_A5},{"A6",M68K_REG_A6},{"A7",M68K_REG_A7},
};

int main(int argc, char **argv)
{
    const char *seed = NULL, *script = NULL, *tracef = NULL;
    long frames = 100, frame;
    long len; char *js, digest[65];
    FILE *out;
    int i, found, sha_ok;
    unsigned int sr;

    for (i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--seed") && i + 1 < argc) seed = argv[++i];
        else if (!strcmp(argv[i], "--script") && i + 1 < argc) script = argv[++i];
        else if (!strcmp(argv[i], "--frames") && i + 1 < argc) frames = atol(argv[++i]);
        else if (!strcmp(argv[i], "--trace") && i + 1 < argc) tracef = argv[++i];
        else if (!strcmp(argv[i], "--frame-cycles") && i + 1 < argc) frame_cycles = atol(argv[++i]);
        else if (!strcmp(argv[i], "--ikbd-delay") && i + 1 < argc) ikbd_delay = atol(argv[++i]);
        else if (!strcmp(argv[i], "--autopilot") && i + 3 < argc) {
            ap_on = 1;
            ap_cell = atoi(argv[++i]);
            ap_fire_period = atoi(argv[++i]);
            ap_fire_from = atoi(argv[++i]);
        }
        else if (!strcmp(argv[i], "--emit-script") && i + 1 < argc) {
            ap_script = fopen(argv[++i], "w");
            if (!ap_script) { perror("--emit-script"); return 2; }
            fprintf(ap_script, "# generated by --autopilot\nj 0 00 00\n");
        }
        else if (!strcmp(argv[i], "--permissive")) permissive = 1;
        else if (!strcmp(argv[i], "--debug-regs")) debug_regs = 1;
        else if (!strcmp(argv[i], "--dump-pcs") && i + 1 < argc) dump_pcs = atol(argv[++i]);
        else if (!strcmp(argv[i], "--window") && i + 2 < argc) {
            win_lo = strtol(argv[++i], NULL, 0); win_hi = strtol(argv[++i], NULL, 0);
        }
        else { fprintf(stderr,
            "usage: %s --seed <f.seed> [--script <f>] [--frames N]\n"
            "          [--trace <out.ndjson>] [--window LO HI]\n"
            "          [--permissive] [--debug-regs]\n", argv[0]);
            return 2; }
    }
    if (!seed) { fprintf(stderr, "--seed is required\n"); return 2; }

    {   /* RAM image */
        char *p; long n;
        FILE *f = fopen(seed, "rb");
        if (!f) { perror(seed); return 2; }
        n = fread(ram, 1, RAM_SIZE, f);
        fclose(f);
        if (n != RAM_SIZE) { fprintf(stderr, "seed is %ld bytes, expected %u\n", n, RAM_SIZE); return 2; }
        p = malloc(strlen(seed) + 6);
        sprintf(p, "%s.json", seed);
        js = slurp(p, &len);
        free(p);
    }

    hexdigest(ram, RAM_SIZE, digest);
    {   /* The seed's own hash must match, or nothing downstream means
         * anything.  Check EVERY "sha256" key, not the first: a relayed seed
         * records its parent's hash too, and matching only the first key made
         * a perfectly good seed look corrupt. */
        const char *q = js;
        sha_ok = 0;
        while ((q = strstr(q, "\"sha256\"")) != NULL) {
            const char *d = strstr(q, digest);
            if (d && d - q < 24) { sha_ok = 1; break; }
            q += 8;
        }
        if (!sha_ok) {
            fprintf(stderr, "seed sha256 mismatch: image hashes to %s\n", digest);
            return 2;
        }
    }

    m68k_set_cpu_type(M68K_CPU_TYPE_68000);
    m68k_init();
    m68k_pulse_reset();
    for (i = 0; i < 16; i++) {
        long v = json_int(js, REGS[i].n, &found);
        if (!found) { fprintf(stderr, "seed json lacks %s\n", REGS[i].n); return 2; }
        m68k_set_reg(REGS[i].r, (unsigned)v);
    }
    sr = (unsigned)json_int(js, "SR", &found);
    if (!found) { fprintf(stderr, "seed json lacks SR\n"); return 2; }
    m68k_set_reg(M68K_REG_SR, sr);           /* before PC: sets the mask and S bit */
    m68k_set_reg(M68K_REG_USP, (unsigned)json_int(js, "USP", &found));
    m68k_set_reg(M68K_REG_ISP, (unsigned)json_int(js, "ISP", &found));
    m68k_set_reg(M68K_REG_PC, (unsigned)json_int(js, "PC", &found));

    /* MFP shadow: a sound effect may already be mid-stream at the seed
     * instant, in which case Timer A has to be ticking from frame 0 or the
     * $6c5b/$6c5c busy latch never clears. */
    mfp_tadr = (uint8_t)json_int(js, "TADR", &found);
    if (!found) { fprintf(stderr, "seed json lacks TADR (re-capture the seed)\n"); return 2; }
    mfp_tbdr = (uint8_t)json_int(js, "TBDR", &found);
    mfp_tbcr = (uint8_t)json_int(js, "TBCR", &found);
    mfp_tacr = (uint8_t)json_int(js, "TACR", &found);
    if (mfp_tacr & 7) tima_reload();

    if (m68k_get_reg(NULL, M68K_REG_PC) != VBL_HANDLER) {
        fprintf(stderr, "seed PC is $%x, expected the VBL handler $%x\n",
                m68k_get_reg(NULL, M68K_REG_PC), VBL_HANDLER);
        return 2;
    }

    load_script(script);
    out = tracef ? fopen(tracef, "w") : stdout;
    if (!out) { perror(tracef); return 2; }

    if (dump_pcs) {
        long n;
        unsigned prev = 0; long run = 0;
        for (n = 0; n < dump_pcs; n++) {
            unsigned pc = m68k_get_reg(NULL, M68K_REG_PC) & ADDR_MASK;
            if (pc == prev) run++;
            else {
                if (run) fprintf(stderr, "   (x%ld)\n", run + 1);
                else if (n) fprintf(stderr, "\n");
                fprintf(stderr, "$%06x sr=$%04x", pc, m68k_get_reg(NULL, M68K_REG_SR));
                run = 0;
            }
            prev = pc;
            m68k_execute(1);
        }
        fprintf(stderr, "\n");
        return 0;
    }
    trace_out = out;
    /* The seed already sits at the sampling point, so frame 0 is emitted
       directly; every later frame is caught by the hook. */
    cur_frame = 0;
    emit_frame(out, 0);
    apply_events(0);
    for (frame = 1; frame < frames; frame++) {
        long guard = 0;
        run_cycles(frame_cycles);
        vbl_pending = 1;
        refresh_irq();
        cur_frame = frame;
        sample_armed = 1; sampled = 0;
        while (!sampled && guard++ < 20000) { cycles_now += m68k_execute(200); }
        if (!sampled) {
            fprintf(stderr, "frame %ld: never reached $%x after the VBL; "
                    "stuck at pc=$%06x sr=$%04x\n", frame, VBL_HANDLER,
                    m68k_get_reg(NULL, M68K_REG_PC) & ADDR_MASK,
                    m68k_get_reg(NULL, M68K_REG_SR));
            return 5;
        }
        apply_events(frame);
    }
    if (tracef) fclose(out);
    if (ap_script) fclose(ap_script);
    if (unstubbed)
        fprintf(stderr, "note: %ld unstubbed I/O accesses were forgiven (--permissive)\n",
                unstubbed);
    return 0;
}
