// SPDX-License-Identifier: GPL-3.0-or-later
using System;
using System.IO;
using System.Linq;
using Mono.Cecil;
using Mono.Cecil.Cil;

internal static class Patcher
{
    public static int Main(string[] args)
    {
        if (args.Length == 1 && args[0] == "--version") { Console.WriteLine("g13map-terraria-patcher 0.1.0"); return 0; }
        if (args.Length < 3) { Console.Error.WriteLine("usage: patcher ORIGINAL COPY OBSERVER_DLL [ASSEMBLY_DIR...]"); return 2; }
        string created = null;
        try {
            string original = Path.GetFullPath(args[0]), output = Path.GetFullPath(args[1]);
            if (original == output || File.Exists(output)) throw new InvalidOperationException("Output must be a new copy, never the original.");
            using (var resolver = new EmbeddedResolver()) {
            resolver.AddSearchDirectory(Path.GetDirectoryName(original));
            foreach (string directory in args.Skip(3)) resolver.AddSearchDirectory(directory);
            using (var game = AssemblyDefinition.ReadAssembly(original, new ReaderParameters { AssemblyResolver = resolver }))
            using (var helper = AssemblyDefinition.ReadAssembly(args[2])) {
                resolver.Game = game.MainModule;
                if (game.Name.Name != "Terraria" || game.Name.HasPublicKey) throw new InvalidOperationException("Expected an unsigned Terraria assembly.");
                if (game.MainModule.AssemblyReferences.Any(r => r.Name == helper.Name.Name)) throw new InvalidOperationException("Already patched.");
                var main = game.MainModule.Types.Single(t => t.FullName == "Terraria.Main");
                var update = main.Methods.Single(m => m.Name == "Update" && !m.IsStatic && m.HasBody && m.ReturnType.FullName == "System.Void" && m.Parameters.Count == 1 && m.Parameters[0].ParameterType.FullName == "Microsoft.Xna.Framework.GameTime");
                var observer = helper.MainModule.Types.Single(t => t.FullName == "G13TerrariaVanilla.Observer").Methods.Single(m => m.Name == "Sample");
                var call = game.MainModule.ImportReference(observer);
                var returns = update.Body.Instructions.Where(i => i.OpCode == OpCodes.Ret).ToArray();
                if (returns.Length == 0) throw new InvalidOperationException("Update has no normal return.");
                var il = update.Body.GetILProcessor();
                // Keep branch targets on the existing instruction so every normal
                // return executes the observer. Widen short branches after insertion.
                foreach (var ret in returns) {
                    ret.OpCode = OpCodes.Ldarg_0;
                    var invoke = il.Create(OpCodes.Call, call);
                    il.InsertAfter(ret, invoke);
                    il.InsertAfter(invoke, il.Create(OpCodes.Ret));
                }
                foreach (var instruction in update.Body.Instructions) {
                    if (instruction.OpCode.OperandType == OperandType.ShortInlineBrTarget) {
                        var longer = typeof(OpCodes).GetFields().Select(f => (OpCode)f.GetValue(null)).Single(o => o.Name == instruction.OpCode.Name.Substring(0, instruction.OpCode.Name.Length - 2));
                        instruction.OpCode = longer;
                    }
                }
                created = output;
                game.Write(output);
                Console.WriteLine("Terraria " + game.Name.Version + ": observed " + returns.Length + " Update return(s); original preserved.");
            }
            }
            return 0;
        }
        catch (Exception error) {
            if (created != null) {
                try { File.Delete(created); } catch (IOException) { } catch (UnauthorizedAccessException) { }
            }
            Console.Error.WriteLine(error.Message); return 1;
        }
    }

    private sealed class EmbeddedResolver : DefaultAssemblyResolver
    {
        public ModuleDefinition Game;
        public override AssemblyDefinition Resolve(AssemblyNameReference name)
        {
            var resource = Game == null ? null : Game.Resources.OfType<EmbeddedResource>().FirstOrDefault(r => r.Name.EndsWith("." + name.Name + ".dll", StringComparison.Ordinal));
            if (resource != null) return AssemblyDefinition.ReadAssembly(resource.GetResourceStream(), new ReaderParameters { AssemblyResolver = this });
            return base.Resolve(name);
        }
    }
}
