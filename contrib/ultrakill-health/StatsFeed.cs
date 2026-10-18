// SPDX-License-Identifier: GPL-3.0-or-later
using System;
using System.Globalization;
using System.IO;
using BepInEx.Logging;

namespace G13Health
{
    internal static class StatsFeed
    {
        static string path;
        static string error;
        static string pending = "wait ttl 3";
        static string last;
        static int pendingAt;
        static int writtenAt;
        static readonly object gate = new object();
        static ManualLogSource log;
        static Accuracy level;
        static uint deaths;
        static bool dead;

        internal static void Init(string destination, string healthPath, ManualLogSource logger)
        {
            log = logger;
            try
            {
                if (string.Equals(Path.GetFullPath(destination), Path.GetFullPath(healthPath), StringComparison.OrdinalIgnoreCase))
                    throw new ArgumentException("health and combat need different paths");
                Directory.CreateDirectory(Path.GetDirectoryName(destination));
                path = destination;
                log.LogInfo("combat stats feed: " + path);
            }
            catch (Exception e) { log.LogWarning("combat stats feed disabled: " + e.Message); }
        }
        internal static void Publish(NewMovement player, string rank)
        {
            if (path == null) return;
            if (!ReferenceEquals(level, CombatTracking.Level))
            {
                level = CombatTracking.Level;
                deaths = 0;
                dead = false;
            }
            if (player.dead && !dead) deaths++;
            dead = player.dead;
            var stats = StatsManager.Instance;
            string line = stats == null ? "wait ttl 3" : string.Format(CultureInfo.InvariantCulture,
                "combat 1 hp {0} shots {1} hits {2} style {3} time {4:F3} deaths {5} rank {6} known {7} ttl 3",
                player.dead ? 0 : Math.Max(0, player.hp), level.Shots, level.Hits,
                Math.Max(0, stats.stylePoints), Math.Max(0, stats.seconds), deaths, rank,
                level.Available ? 1 : 0);
            lock (gate) { pending = line; pendingAt = Environment.TickCount; Write(line); }
        }
        internal static void Keepalive()
        {
            if (path == null) return;
            lock (gate) { Write(Environment.TickCount - pendingAt > 1500 ? "wait ttl 3" : pending); }
        }
        static void Write(string line)
        {
            int now = Environment.TickCount;
            if (line == last && now - writtenAt < 1000) return;
            try
            {
                File.WriteAllText(path + ".new", line + "\n");
                if (File.Exists(path)) File.Replace(path + ".new", path, null);
                else File.Move(path + ".new", path);
                last = line;
                writtenAt = now;
            }
            catch (Exception e)
            {
                if (error != e.Message) { error = e.Message; log.LogWarning("combat stats write: " + error); }
            }
        }
        internal static void Close()
        {
            if (path == null) return;
            lock (gate) { try { File.Delete(path); } catch (Exception) { } }
        }
    }
}
