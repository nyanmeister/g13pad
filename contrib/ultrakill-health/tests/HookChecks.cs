// SPDX-License-Identifier: GPL-3.0-or-later
// Real Harmony patches, fake Unity objects: verifies the observation mechanism
// without loading a game or writing to a desktop/G13. Live Unity remains separate.
using System;
using System.Runtime.CompilerServices;
using G13Health;
using UnityEngine;
using Object = UnityEngine.Object;

namespace BepInEx.Logging {
    public class ManualLogSource {
        public void LogInfo(object value) { Console.WriteLine(value); }
        public void LogWarning(object value) { Console.WriteLine(value); }
    }
}
namespace UnityEngine {
    public class Object {
        [MethodImpl(MethodImplOptions.NoInlining)]
        public static GameObject Instantiate(GameObject prefab) { return prefab.factory(); }
    }
    public class Component : Object { public GameObject gameObject; }
    public class Transform : Component { public Transform parent; }
    public class GameObject : Object {
        public Func<GameObject> factory;
        public Component carrier;
        public Transform transform;
        public GameObject() { transform = new Transform { gameObject = this }; }
    }
}
public class EnemyIdentifier {
    public bool dead;
    public float Health = 100;
    [MethodImpl(MethodImplOptions.NoInlining)]
    public void DeliverDamage() { Health = Math.Max(0, Health - 5); }
}
public class StatsManager {
    public int starts;
    [MethodImpl(MethodImplOptions.NoInlining)]
    public void Awake() { starts++; }
}
public class Revolver : Component {
    public GameObject prefab, last;
    [MethodImpl(MethodImplOptions.NoInlining)]
    public void Shoot(int shotType) { last = Object.Instantiate(prefab); if (shotType == 99) throw new InvalidOperationException("game exception"); }
    [MethodImpl(MethodImplOptions.NoInlining)]
    public void ThrowCoin() { last = Object.Instantiate(prefab); }
}
public class RevolverBeam : Component {
    [MethodImpl(MethodImplOptions.NoInlining)]
    public void Hit(EnemyIdentifier enemy) { enemy.DeliverDamage(); }
    [MethodImpl(MethodImplOptions.NoInlining)]
    public void HitCoin(Coin coin) { coin.DelayedReflectRevolver(); }
    [MethodImpl(MethodImplOptions.NoInlining)]
    public void Detonate(Grenade core) { core.Explode(); }
}
public class Coin : Component {
    public GameObject prefab, reflected;
    [MethodImpl(MethodImplOptions.NoInlining)]
    public void DelayedReflectRevolver() { }
    [MethodImpl(MethodImplOptions.NoInlining)]
    public void ReflectRevolver() { reflected = Object.Instantiate(prefab); }
}
public class Shotgun : Component {
    public GameObject prefab;
    public GameObject[] pellets;
    [MethodImpl(MethodImplOptions.NoInlining)]
    public void Shoot() { pellets = new[] { Object.Instantiate(prefab), Object.Instantiate(prefab), Object.Instantiate(prefab) }; }
    [MethodImpl(MethodImplOptions.NoInlining)]
    public void ShootSinks() { pellets = new[] { Object.Instantiate(prefab) }; }
    public void ShootSaw(bool noSaw) { if (!noSaw) Shoot(); }
}
public class ShotgunHammer : Component {
    public void Impact() { }
    public void ThrowNade() { }
    public void ShootSaw(bool noSaw) { }
}
public class Nailgun : Component {
    public void Shoot() { }
    public void BurstFire() { }
    public void SuperSaw() { }
    public void ShootZapper() { }
}
public class Railcannon : Component {
    public GameObject prefab, last;
    [MethodImpl(MethodImplOptions.NoInlining)]
    public void Shoot() { last = Object.Instantiate(prefab); }
}
public class RocketLauncher : Component {
    public void Shoot() { }
    public void ShootCannonball() { }
    public void ShootNapalm() { }
}
public class Grenade : Component {
    public GameObject prefab, explosion;
    [MethodImpl(MethodImplOptions.NoInlining)]
    public void Explode() { explosion = Object.Instantiate(prefab); }
}
public class Explosion : Component {
    [MethodImpl(MethodImplOptions.NoInlining)]
    public void Hit(EnemyIdentifier enemy) { enemy.DeliverDamage(); }
}
public class Projectile : Component {
    [MethodImpl(MethodImplOptions.NoInlining)]
    public void Hit(EnemyIdentifier enemy) { enemy.DeliverDamage(); }
}
static class HookChecks {
    static GameObject Prefab<T>() where T : Component, new() {
        return new GameObject { factory = () => {
            var obj = new GameObject(); obj.carrier = new T { gameObject = obj }; return obj;
        }};
    }
    static void Check(ulong shots, ulong hits, string name) {
        if (!CombatTracking.Level.Available || CombatTracking.Level.Shots != shots || CombatTracking.Level.Hits != hits)
            throw new Exception(name + ": expected " + hits + "/" + shots + ", got " + CombatTracking.Level.Hits + "/" + CombatTracking.Level.Shots);
    }
    static void Main() {
        CombatTracking.Install(new BepInEx.Logging.ManualLogSource());
        Check(0, 0, "hooks installed");
        var enemy = new EnemyIdentifier();
        var revolver = new Revolver { prefab = Prefab<RevolverBeam>() };
        revolver.Shoot(0); // Miss.
        revolver.Shoot(0);
        ((RevolverBeam)revolver.last.carrier).Hit(enemy);
        ((RevolverBeam)revolver.last.carrier).Hit(enemy);
        Check(2, 1, "two revolver shots, repeated damage");

        var shotgun = new Shotgun { prefab = Prefab<Projectile>() };
        shotgun.Shoot();
        foreach (var pellet in shotgun.pellets) ((Projectile)pellet.carrier).Hit(enemy);
        Check(3, 2, "one shotgun action, three pellets");
        shotgun.ShootSaw(true);
        Check(3, 2, "empty saw animation is neutral");

        var coinThrower = new Revolver { prefab = Prefab<Coin>() };
        coinThrower.ThrowCoin();
        var coin = (Coin)coinThrower.last.carrier;
        coin.prefab = Prefab<RevolverBeam>();
        Check(3, 2, "coin throw is neutral");
        revolver.Shoot(0);
        ((RevolverBeam)revolver.last.carrier).HitCoin(coin);
        Check(4, 2, "hitting coin alone is not a hit");
        coin.ReflectRevolver(); // Later callback, after the original stack unwound.
        ((RevolverBeam)coin.reflected.carrier).Hit(enemy);
        Check(4, 3, "delayed coin chain connects");

        shotgun.prefab = Prefab<Grenade>();
        shotgun.ShootSinks();
        var core = (Grenade)shotgun.pellets[0].carrier;
        core.prefab = Prefab<Explosion>();
        var rail = new Railcannon { prefab = Prefab<RevolverBeam>() };
        rail.Shoot();
        ((RevolverBeam)rail.last.carrier).Detonate(core);
        ((Explosion)core.explosion.carrier).Hit(enemy);
        ((Explosion)core.explosion.carrier).Hit(enemy);
        Check(6, 5, "delayed nuke blast credits both contributing shots once");

        try { revolver.Shoot(99); throw new Exception("lost game exception"); }
        catch (InvalidOperationException e) { if (e.Message != "game exception") throw; }
        var untracked = new RevolverBeam { gameObject = new GameObject() };
        untracked.Hit(enemy);
        Check(7, 5, "context restored after game exception; untracked damage neutral");
        var old = (RevolverBeam)revolver.last.carrier;
        new StatsManager().Awake();
        old.Hit(enemy);
        Check(0, 0, "old projectile cannot enter new level ledger");
        Console.WriteLine("PASS: real Harmony spawn attribution, pellets, coin delay, nuke, exceptions and level reset");
    }
}
