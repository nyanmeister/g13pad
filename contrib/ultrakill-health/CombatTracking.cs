// SPDX-License-Identifier: GPL-3.0-or-later
using System;
using System.Collections.Generic;
using System.Reflection;
using System.Reflection.Emit;
using System.Runtime.CompilerServices;
using BepInEx.Logging;
using HarmonyLib;
using UnityEngine;
using Cause = G13Health.Accuracy.Cause;
using Object = UnityEngine.Object;

namespace G13Health
{
    // These hooks observe attack creation/damage; they do not replace game logic.
    // Metadata lives in weak tables, never in added components or game fields.
    internal static class CombatTracking
    {
        internal static Accuracy Level = new Accuracy();
        static readonly ConditionalWeakTable<object, Cause> causes = new ConditionalWeakTable<object, Cause>();
        static readonly Dictionary<Type, FieldInfo> iteratorOwners = new Dictionary<Type, FieldInfo>();
        static readonly HashSet<MethodBase> transfers = new HashSet<MethodBase>();
        [ThreadStatic] static Cause current;
        static ManualLogSource log;
        static bool healthy = true;

        static readonly string[] Roots = {
            "Revolver:Shoot", "Shotgun:Shoot", "Shotgun:ShootSinks", "Shotgun:ShootSaw",
            "ShotgunHammer:Impact", "ShotgunHammer:ThrowNade", "ShotgunHammer:ShootSaw",
            "Nailgun:Shoot", "Nailgun:BurstFire", "Nailgun:SuperSaw", "Nailgun:ShootZapper",
            "Railcannon:Shoot", "RocketLauncher:Shoot", "RocketLauncher:ShootCannonball",
            "RocketLauncher:ShootNapalm"
        };
        static readonly string[] Carriers = {
            "RevolverBeam", "Projectile", "Nail", "Grenade", "Explosion", "Cannonball",
            "Harpoon", "Chainsaw", "Coin", "ContinuousBeam", "PhysicalShockwave", "Zapper",
            "GasolineProjectile", "GasolineStain", "StainVoxel", "VoxelProxy", "BurningVoxel",
            "FireZone", "Flammable"
        };

        internal static void Install(ManualLogSource logger)
        {
            log = logger;
            var harmony = new Harmony("g13pad.stats");
            var patched = new HashSet<MethodBase>();
            try
            {
                FindTransfers();
                foreach (var entry in Roots)
                {
                    var pair = entry.Split(':');
                    var type = AccessTools.TypeByName(pair[0]);
                    if (type == null) throw new MissingMemberException(pair[0]);
                    bool found = false;
                    foreach (var method in AccessTools.GetDeclaredMethods(type))
                    {
                        if (method.Name != pair[1] || method.GetMethodBody() == null) continue;
                        // The zero-argument saw method just calls the bool overload.
                        if (method.Name == "ShootSaw" && method.GetParameters().Length == 0) continue;
                        Patch(harmony, method, true);
                        patched.Add(method);
                        found = true;
                    }
                    if (!found) throw new MissingMethodException(entry);
                }
                foreach (var name in Carriers)
                {
                    var type = AccessTools.TypeByName(name);
                    if (type == null) continue; // Optional in other game revisions.
                    PatchCarrier(harmony, type, patched);
                }
                // Hammer impact is a coroutine. The delayed damage/effects inherit
                // the cause saved on the weapon by Impact, rather than counting anew.
                var hammer = AccessTools.TypeByName("ShotgunHammer");
                foreach (var method in AccessTools.GetDeclaredMethods(hammer))
                    if ((method.Name == "DeliverDamage" || method.Name == "HitNade" || method.Name == "ImpactEffects") && patched.Add(method))
                        Patch(harmony, method, false);
                PatchIterators(harmony, hammer, patched);
                harmony.Patch(AccessTools.Method(typeof(EnemyIdentifier), "DeliverDamage"),
                    prefix: Hook("BeforeDamage"), postfix: Hook("AfterDamage"));
                harmony.Patch(AccessTools.Method(typeof(StatsManager), "Awake"), postfix: Hook("NewLevel"));
                log.LogInfo("accuracy hooks ready: " + patched.Count + " attack/carrier methods");
            }
            catch (Exception e)
            {
                Fail(e);
                harmony.UnpatchSelf();
            }
        }

        static HarmonyMethod Hook(string name) { return new HarmonyMethod(typeof(CombatTracking), name); }
        static void Patch(Harmony harmony, MethodBase method, bool root)
        {
            harmony.Patch(method, prefix: Hook(root ? "BeginAttack" : "EnterCarrier"),
                transpiler: Hook("Spawns"), finalizer: Hook("Leave"));
        }
        static void PatchCarrier(Harmony harmony, Type type, HashSet<MethodBase> patched)
        {
            foreach (var method in AccessTools.GetDeclaredMethods(type))
                if (!method.IsStatic && !method.IsSpecialName && method.GetMethodBody() != null && NeedsScope(method) && patched.Add(method))
                    Patch(harmony, method, false);
            PatchIterators(harmony, type, patched);
        }
        static bool NeedsScope(MethodBase method)
        {
            // An incoming cross-object call may only schedule a later callback.
            // Preserve its cause even when the callee itself spawns/damages nothing.
            if (transfers.Contains(method)) return true;
            // Select once at startup. Per-frame movement/visual methods need no
            // scope when their damage/spawn helper already establishes its own.
            foreach (var instruction in PatchProcessor.GetOriginalInstructions(method))
            {
                var called = instruction.operand as MethodBase;
                if (called == null || called.DeclaringType == null) continue;
                var type = called.DeclaringType;
                if (type == typeof(Object) && called.Name == "Instantiate") return true;
                if (type.Name == "EnemyIdentifier" || type.Name == "StainVoxelManager") return true;
                if (type != method.DeclaringType && Array.IndexOf(Carriers, type.Name) >= 0) return true;
            }
            return false;
        }
        static void FindTransfers()
        {
            var types = new HashSet<Type>();
            foreach (var name in Carriers)
            {
                var type = AccessTools.TypeByName(name);
                if (type != null) types.Add(type);
            }
            foreach (var entry in Roots)
            {
                var type = AccessTools.TypeByName(entry.Split(':')[0]);
                if (type != null) types.Add(type);
            }
            foreach (var type in new List<Type>(types))
                foreach (var nested in type.GetNestedTypes(BindingFlags.Public | BindingFlags.NonPublic)) types.Add(nested);
            foreach (var type in types)
                foreach (var method in AccessTools.GetDeclaredMethods(type))
                {
                    if (method.GetMethodBody() == null) continue;
                    foreach (var instruction in PatchProcessor.GetOriginalInstructions(method))
                    {
                        var called = instruction.operand as MethodInfo;
                        if (called != null && called.DeclaringType != type && called.DeclaringType != null
                            && Array.IndexOf(Carriers, called.DeclaringType.Name) >= 0) transfers.Add(called);
                    }
                }
        }
        static void PatchIterators(Harmony harmony, Type type, HashSet<MethodBase> patched)
        {
            foreach (var nested in type.GetNestedTypes(BindingFlags.Public | BindingFlags.NonPublic))
            {
                var owner = nested.GetField("<>4__this", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
                var move = AccessTools.DeclaredMethod(nested, "MoveNext");
                if (owner == null || move == null || !NeedsScope(move) || !patched.Add(move)) continue;
                iteratorOwners[nested] = owner;
                Patch(harmony, move, false);
            }
        }

        static GameObject GameObjectOf(object instance)
        {
            var component = instance as Component;
            if (component != null) return component.gameObject;
            FieldInfo owner;
            if (instance != null && iteratorOwners.TryGetValue(instance.GetType(), out owner))
                return GameObjectOf(owner.GetValue(instance));
            return instance as GameObject;
        }
        static object Key(object instance) { return (object)GameObjectOf(instance) ?? instance; }
        static Cause Find(object instance)
        {
            var obj = GameObjectOf(instance);
            if (obj == null)
            {
                Cause value;
                return instance != null && causes.TryGetValue(instance, out value) ? value : null;
            }
            for (var transform = obj.transform; transform != null; transform = transform.parent)
            {
                Cause cause;
                if (causes.TryGetValue(transform.gameObject, out cause)) return cause;
            }
            return null;
        }
        static void Bind(object instance, Cause cause)
        {
            var key = Key(instance);
            if (key == null || cause == null) return;
            causes.Remove(key);
            causes.Add(key, cause);
        }
        static void BeginAttack(object __instance, MethodBase __originalMethod, object[] __args, out Cause __state)
        {
            __state = current;
            if (!healthy || current != null) return;
            // A noSaw animation is not an attack. Utility coin/magnet throws and
            // rocket-freeze toggles have no firing hook in the first place.
            if (__originalMethod.Name == "ShootSaw" && __args.Length == 1 && (bool)__args[0]) return;
            try
            {
                current = Level.Fire();
                if (__originalMethod.DeclaringType.Name == "ShotgunHammer" && __originalMethod.Name == "Impact")
                    Bind(GameObjectOf(__instance), current);
            }
            catch (Exception e) { Fail(e); current = null; }
        }
        static void EnterCarrier(object __instance, out Cause __state)
        {
            __state = current;
            if (!healthy) return;
            try
            {
                var obj = Key(__instance);
                current = Cause.Merge(current, Find(obj));
                Bind(obj, current);
            }
            catch (Exception e) { Fail(e); current = null; }
        }
        static Exception Leave(Exception __exception, Cause __state)
        {
            current = __state;
            return __exception; // Preserve the game's exception, if any.
        }
        static void Stamp(Object spawned)
        {
            if (current == null || !healthy) return;
            try { Bind(spawned, current); }
            catch (Exception e) { Fail(e); }
        }
        static IEnumerable<CodeInstruction> Spawns(IEnumerable<CodeInstruction> instructions)
        {
            // Keep every original operation, including its labels/exception blocks.
            // Duplicate only the Instantiate result to attach weak attribution.
            foreach (var instruction in instructions)
            {
                yield return instruction;
                var method = instruction.operand as MethodInfo;
                if ((instruction.opcode == OpCodes.Call || instruction.opcode == OpCodes.Callvirt)
                    && method != null && method.DeclaringType == typeof(Object) && method.Name == "Instantiate"
                    && typeof(Object).IsAssignableFrom(method.ReturnType))
                {
                    yield return new CodeInstruction(OpCodes.Dup);
                    yield return new CodeInstruction(OpCodes.Call, AccessTools.Method(typeof(CombatTracking), "Stamp"));
                }
            }
        }
        static void BeforeDamage(EnemyIdentifier __instance, out float __state)
        {
            __state = 0;
            try { if (current != null && !__instance.dead) __state = __instance.Health; }
            catch (Exception e) { Fail(e); }
        }
        static void AfterDamage(EnemyIdentifier __instance, float __state)
        {
            try
            {
                if (healthy && current != null && __state > 0 && __instance.Health < __state)
                    current.Connect();
            }
            catch (Exception e) { Fail(e); }
        }
        static void NewLevel() { Level = new Accuracy { Available = healthy }; }
        static void Fail(Exception e)
        {
            healthy = false;
            Level.Available = false;
            if (log != null) log.LogWarning("accuracy unavailable: " + e);
        }
    }
}
