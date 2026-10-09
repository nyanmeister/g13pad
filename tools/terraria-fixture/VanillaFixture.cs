// SPDX-License-Identifier: GPL-3.0-or-later
// A fake CLR game for the observer/IL boundary. Never install in a real game.
using System;
using System.IO;
using System.Threading;
namespace Microsoft.Xna.Framework { public struct GameTime { } }
namespace Terraria
{
    public class Player
    {
        public bool active = true, dead;
        public int statLife = 240, statLifeMax2 = 400, statMana = 80, statManaMax2 = 200;
        public int statDefense = 45, breath = 80, breathMax = 200;
    }
    public class Main
    {
        public static bool gameMenu;
        public static int myPlayer;
        public static Player[] player = { new Player() };
        public int calls;
        public void Update(Microsoft.Xna.Framework.GameTime time)
        {
            calls++;
            if (calls == 1) return;
            player[0].statMana = 0;
        }
    }
    internal static class Entry
    {
        public static int Main(string[] args)
        {
            var game = new Terraria.Main();
            game.Update(new Microsoft.Xna.Framework.GameTime());
            string path = Environment.GetEnvironmentVariable("G13MAP_HEALTH_FILE");
            if (Path.DirectorySeparatorChar == '\\' && path.StartsWith("/")) path = "Z:" + path.Replace('/', '\\');
            Check(path, "240/400 mana 80/200 defense 45 breath 80/200 ttl 3");
            game.Update(new Microsoft.Xna.Framework.GameTime());
            Thread.Sleep(150);
            game.Update(new Microsoft.Xna.Framework.GameTime());
            Check(path, "240/400 mana 0/200 defense 45 breath 80/200 ttl 3");
            long first = File.GetLastWriteTimeUtc(path).Ticks;
            Thread.Sleep(1400); // no game updates: heartbeat must survive real pause.
            if (File.GetLastWriteTimeUtc(path).Ticks <= first) throw new Exception("Paused feed did not refresh");
            Terraria.Main.player[0].dead = true;
            game.Update(new Microsoft.Xna.Framework.GameTime());
            Check(path, "0/400 mana 0/200 defense 45 breath 80/200 ttl 3");
            Thread.Sleep(600);
            Terraria.Main.gameMenu = true;
            game.Update(new Microsoft.Xna.Framework.GameTime());
            Check(path, "wait ttl 3");
            if (game.calls != 5) throw new Exception("Changed original Update behavior");
            Console.WriteLine("Observer: health/mana/maxima/defense/air, paused heartbeat, death/menu, original Update PASS");
            return 0;
        }
        private static void Check(string path, string expected)
        {
            for (int i = 0; i < 30; i++) {
                Thread.Sleep(100);
                if (File.Exists(path) && File.ReadAllText(path).Trim() == expected) return;
            }
            throw new Exception("Expected feed: " + expected);
        }
    }
}
