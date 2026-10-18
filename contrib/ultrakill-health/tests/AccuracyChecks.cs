// SPDX-License-Identifier: GPL-3.0-or-later
using System;
using G13Health;

static class AccuracyChecks
{
    static void Check(Accuracy ledger, ulong shots, ulong hits, string name)
    {
        if (ledger.Shots != shots || ledger.Hits != hits)
            throw new Exception(name + ": expected " + hits + "/" + shots + ", got " + ledger.Hits + "/" + ledger.Shots);
    }
    static void Main()
    {
        var level = new Accuracy();
        Check(level, 0, 0, "no shots");
        level.Fire(); // One deliberate miss.
        var pellets = level.Fire();
        for (int pellet = 0; pellet < 12; pellet++) pellets.Connect();
        Check(level, 2, 1, "one shotgun action, twelve pellet hits");

        var core = level.Fire();
        var rail = level.Fire();
        var nuke = Accuracy.Cause.Merge(core, rail);
        nuke.Connect(); nuke.Connect();
        Check(level, 4, 3, "core and rail, multiple blast victims");

        var coinShot = level.Fire();
        var split = Accuracy.Cause.Merge(coinShot, coinShot);
        split.Connect(); coinShot.Connect();
        Check(level, 5, 4, "one shot through split coins");

        var oldProjectile = level.Fire();
        var next = new Accuracy();
        oldProjectile.Connect();
        Check(next, 0, 0, "late damage stays in old level");
        Check(level, 6, 5, "old projectile still belongs to its level");

        // Independent oracle: a boolean per firing action, never the cause graph.
        int seed = Environment.TickCount;
        Console.WriteLine("random seed " + seed);
        var random = new Random(seed);
        var generated = new Accuracy();
        var attacks = new Accuracy.Cause[257];
        var expected = new bool[257];
        for (int i = 0; i < attacks.Length; i++) attacks[i] = generated.Fire();
        for (int i = 0; i < 10000; i++)
        {
            int a = random.Next(attacks.Length), b = random.Next(attacks.Length);
            var cause = Accuracy.Cause.Merge(attacks[a], attacks[b]);
            if (random.Next(4) == 0)
            {
                if (cause != null) cause.Connect();
                expected[a] = expected[b] = true;
            }
            ulong intermediate = 0;
            foreach (bool hit in expected) if (hit) intermediate++;
            Check(generated, 257, intermediate, "random merge step " + i);
        }
        ulong hits = 0;
        foreach (bool hit in expected) if (hit) hits++;
        Check(generated, 257, hits, "random cause merges and repeated connections");
        Console.WriteLine("PASS: misses, pellets, nuke, coins, scene ownership, randomized causal roots");
    }
}
