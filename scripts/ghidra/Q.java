// Query the analysed RAM image.  Args are a sequence of commands:
//   xref <hexaddr>      every reference to that address, with the instruction
//   dec  <hexaddr>      decompiled C of the function containing it
//   dis  <hexaddr> <n>  n instructions from there
//   fun                 list every function
//   scan <hexaddr>      every instruction anywhere whose operands mention it
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import ghidra.program.model.scalar.Scalar;

public class Q extends GhidraScript {
    String at(Address a) {
        Instruction i = getInstructionAt(a);
        Function f = getFunctionContaining(a);
        String fn = f == null ? "?" : f.getName();
        return String.format("%-10s [%s]  %s", a, fn, i == null ? "(no instr)" : i.toString());
    }

    void xref(Address t) {
        println("=== XREF to " + t + " ===");
        ReferenceIterator it = currentProgram.getReferenceManager().getReferencesTo(t);
        int n = 0;
        while (it.hasNext()) {
            Reference r = it.next();
            println(String.format("  %-14s %s", r.getReferenceType().getName(), at(r.getFromAddress())));
            n++;
        }
        println("  (" + n + " reference(s))");
    }

    void scan(long target) {
        println("=== SCAN for operand " + Long.toHexString(target) + " ===");
        InstructionIterator it = currentProgram.getListing().getInstructions(true);
        int n = 0;
        while (it.hasNext()) {
            Instruction i = it.next();
            boolean hit = false;
            for (int op = 0; op < i.getNumOperands() && !hit; op++) {
                for (Object o : i.getOpObjects(op)) {
                    if (o instanceof Scalar && ((Scalar) o).getUnsignedValue() == target) hit = true;
                    if (o instanceof Address && ((Address) o).getOffset() == target) hit = true;
                }
            }
            if (hit) { println("  " + at(i.getAddress())); n++; }
        }
        println("  (" + n + " instruction(s))");
    }

    void dec(Address a) throws Exception {
        Function f = getFunctionContaining(a);
        if (f == null) { println("=== no function at " + a + " ==="); return; }
        println("=== DECOMPILE " + f.getName() + " @ " + f.getEntryPoint() + " ===");
        DecompInterface d = new DecompInterface();
        d.openProgram(currentProgram);
        DecompileResults r = d.decompileFunction(f, 120, monitor);
        if (r.decompileCompleted()) println(r.getDecompiledFunction().getC());
        else println("(decompile failed: " + r.getErrorMessage() + ")");
        d.dispose();
    }

    void dis(Address a, int n) {
        println("=== DISASM " + a + " x" + n + " ===");
        Instruction i = getInstructionAt(a);
        if (i == null) i = getInstructionAfter(a);
        for (int k = 0; k < n && i != null; k++) {
            println(String.format("  %-10s %-24s %s", i.getAddress(), bytesOf(i), i.toString()));
            i = i.getNext();
        }
    }

    String bytesOf(Instruction i) {
        StringBuilder sb = new StringBuilder();
        try { for (byte b : i.getBytes()) sb.append(String.format("%02x", b)); } catch (Exception e) {}
        return sb.toString();
    }

    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        for (int i = 0; i < a.length; i++) {
            switch (a[i]) {
                case "xref": xref(toAddr(Long.parseLong(a[++i], 16))); break;
                case "scan": scan(Long.parseLong(a[++i], 16)); break;
                case "dec":  dec(toAddr(Long.parseLong(a[++i], 16))); break;
                case "dis":  dis(toAddr(Long.parseLong(a[++i], 16)), Integer.parseInt(a[++i])); break;
                case "fun": {
                    println("=== FUNCTIONS ===");
                    for (Function f : currentProgram.getFunctionManager().getFunctions(true))
                        println(String.format("  %-10s %-8d %s", f.getEntryPoint(),
                                f.getBody().getNumAddresses(), f.getName()));
                    break;
                }
                default: println("unknown command: " + a[i]);
            }
        }
    }
}
