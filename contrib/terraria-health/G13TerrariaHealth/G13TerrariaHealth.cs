// SPDX-License-Identifier: GPL-3.0-or-later
using System;
using Microsoft.Xna.Framework;
using Terraria;
using Terraria.ModLoader;

namespace G13TerrariaHealth;

public sealed class G13TerrariaHealth : Mod
{
    internal const string HandlerVersion = "0.1.0";
    private HealthFeed feed;

    public override void Load()
    {
        if (Main.dedServ) return;
        try {
            feed = new HealthFeed(HealthFeed.ResolvePath(), message => Logger.Warn(message));
            On_Main.Update += AfterUpdate;
            Logger.Info($"Terraria handler {HandlerVersion}: {feed.Path}");
        }
        catch (Exception error) {
            feed?.Dispose();
            feed = null;
            Logger.Warn($"Terraria handler disabled: {error.Message}");
        }
    }

    private void AfterUpdate(On_Main.orig_Update original, Main game, GameTime time)
    {
        // Let all game and other mod updates complete before reading. Read game
        // objects only on this thread; the writer receives an immutable string.
        original(game, time);
        string line = "wait ttl 3";
        if (!Main.gameMenu && Main.myPlayer >= 0 && Main.myPlayer < Main.player.Length) {
            Player player = Main.player[Main.myPlayer];
            if (player != null && player.active) {
                int health = player.dead ? 0 : Math.Max(0, player.statLife);
                int maximum = Math.Max(1, player.statLifeMax2);
                int maxMana = Math.Max(0, player.statManaMax2);
                int mana = maxMana == 0 ? 0 : Math.Max(0, player.statMana);
                int defense = Math.Max(0, (int)player.statDefense);
                int breath = Math.Max(0, player.breath);
                int maxBreath = Math.Max(1, player.breathMax);
                line = FormattableString.Invariant($"{health}/{maximum} mana {mana}/{maxMana} defense {defense} breath {breath}/{maxBreath} ttl 3");
            }
        }
        feed?.Sample(line);
    }

    public override void Unload()
    {
        On_Main.Update -= AfterUpdate;
        feed?.Dispose();
        feed = null;
    }
}
