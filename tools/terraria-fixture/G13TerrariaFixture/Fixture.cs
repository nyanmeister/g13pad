// SPDX-License-Identifier: GPL-3.0-or-later
// ONLY for isolated copies: changes player stats and launches the first copied world.
using System.Diagnostics;
using Microsoft.Xna.Framework;
using Terraria;
using Terraria.ModLoader;

namespace G13TerrariaFixture;

public sealed class G13TerrariaFixture : Mod
{
    internal static readonly Stopwatch Clock = new();
    private bool started;
    private int reported = -1;

    public override void Load() => On_Main.Update += AfterUpdate;
    public override void Unload() => On_Main.Update -= AfterUpdate;

    private void AfterUpdate(On_Main.orig_Update original, Main game, GameTime time)
    {
        original(game, time);
        if (!started && Main.gameMenu && Main.menuMode == 0) {
            started = true;
            Main.LoadPlayers();
            Main.LoadWorlds();
            Main.ActivePlayerFileData = Main.PlayerList[0];
            Main.myPlayer = 0;
            Main.player[0] = Main.ActivePlayerFileData.Player;
            Main.ActiveWorldFileData = Main.WorldList[0];
            WorldGen.playWorld();
        }
        if (!Clock.IsRunning) return;
        int phase = (int)Clock.Elapsed.TotalSeconds / 4;
        if (phase != reported) {
            reported = phase;
            Logger.Info($"G13 fixture phase {phase}");
        }
        if (phase == 6) {
            // Pause without stopping the main loop: snapshots/heartbeat must survive.
            Main.gamePaused = true;
        }
        if (phase == 7) {
            Main.gameMenu = true;
            Main.menuMode = 0;
        }
        if (phase >= 8) game.Exit();
    }
}

public sealed class FixtureStats : ModSystem
{
    public override void OnWorldLoad() => G13TerrariaFixture.Clock.Restart();

    public override void PostUpdateEverything()
    {
        Player p = Main.LocalPlayer;
        int phase = (int)G13TerrariaFixture.Clock.Elapsed.TotalSeconds / 4;
        // Last update hook supplies real Player fields before the handler reads them.
        p.statLifeMax2 = phase == 2 ? 600 : 400;
        p.statLife = phase switch { 1 => 120, 2 => 300, 3 => 240, 4 => 0, _ => 400 };
        p.statManaMax2 = phase == 2 ? 300 : 200;
        p.statMana = phase switch { 1 => 40, 2 => 120, 3 => 10, 5 => 0, _ => 200 };
        p.statDefense += 45 - (int)p.statDefense;
        p.breathMax = 200;
        p.breath = phase == 3 ? 50 : 200;
        p.dead = phase == 4;
        if (p.dead) p.respawnTimer = 600;
    }
}
