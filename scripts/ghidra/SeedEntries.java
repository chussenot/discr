// Seed the raw RAM image with known 68000 entry points so auto-analysis has
// something to follow.  Addresses come from docs/disc-notes.md.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.mem.MemoryBlock;

public class SeedEntries extends GhidraScript {
    static final long[] ENTRIES = {
        0x8198L, 0x8370L, 0x83d2L, 0x83feL,
        0xa314L, 0xa31cL, 0xa34cL, 0xa354L, 0xa4eaL, 0xa606L, 0xa618L,
        0xa6b2L, 0xa6c2L, 0xa71aL, 0xa722L, 0xa758L, 0xa7d8L, 0xa816L,
        0xa972L, 0xa9a2L, 0xaa50L,
        0xc06eL, 0xc088L,
        0xf5d0L, 0xf5e2L, 0xf7f6L, 0xfe6eL, 0xfe7aL, 0xfbaaL,
        0x10554L, 0x106b2L, 0x108f4L, 0x1094aL, 0x109aaL, 0x10a72L,
        0x10ac4L, 0x10c8aL, 0x10ddaL, 0xfb6eL,
    };

    @Override
    public void run() throws Exception {
        // Mark the whole image executable so the disassembler will work anywhere.
        for (MemoryBlock b : currentProgram.getMemory().getBlocks()) {
            b.setExecute(true);
            b.setWrite(true);
        }
        int n = 0;
        for (long a : ENTRIES) {
            Address addr = toAddr(a);
            try {
                disassemble(addr);
                createFunction(addr, String.format("sub_%x", a));
                n++;
            } catch (Exception e) {
                println("seed failed at " + addr + ": " + e);
            }
        }
        // The player state jump table: 32 longs at $10e2c.
        Address tbl = toAddr(0x10e2cL);
        for (int i = 0; i < 32; i++) {
            Address slot = tbl.add(i * 4L);
            long t = currentProgram.getMemory().getInt(slot) & 0xffffffffL;
            if (t < 0x1000 || t >= 0x100000) continue;
            Address ta = toAddr(t);
            try {
                disassemble(ta);
                createFunction(ta, String.format("state_%02d_%x", i, t));
                n++;
            } catch (Exception e) {
                println("jump-table slot " + i + " failed: " + e);
            }
        }
        println("SeedEntries: seeded " + n + " entry points");
    }
}
