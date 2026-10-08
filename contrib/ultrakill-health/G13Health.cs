// SPDX-License-Identifier: GPL-3.0-or-later
// G13 Health for ULTRAKILL: a BepInEx plugin that writes V1's state to the g13pad
// health meter's feed file ten times a second (asked 2026-10-08).
//
// Line: HP/100 [cap C] rank R style S time T dash D rail R ttl 3
//   cap   the health the bar is capped at: 100 minus hard damage (antiHp)
//   rank  D C B A S SS SSS U, the style rank; style is its meter, percent
//   time  the level timer in seconds; dash the dashes left (0-3); rail the rail charge
// `wait` on the menu (no player), `0` when dead; the ttl ends the meter with the game.
//
// The game never ran this component's Update (found 2026-10-08: three launches, enabled
// and active, not one tick), so the reading rides on the player's own Update through a
// Harmony postfix, and a plain timer thread keeps the file alive between those.
using System;
using System.Globalization;
using System.IO;
using System.Reflection;
using System.Text;
using System.Threading;
using BepInEx;
using BepInEx.Logging;
using HarmonyLib;
using UnityEngine;

namespace G13Health
{
    [BepInPlugin("g13pad.health", "G13 Health", "0.1.1")]
    public class Plugin : BaseUnityPlugin
    {
        static readonly string[] Ranks = { "D", "C", "B", "A", "S", "SS", "SSS", "U" };
        const int PeriodMs = 100;

        static ManualLogSource log;
        static readonly object gate = new object();
        static string path;
        static string tmp;
        static string pending = "wait ttl 3";
        static int pendingAt;
        static string lastLine;
        static int lastWrite;
        static string lastError;
        static int composeAt;
        static int ticks;
        static FieldInfo rankIndex;
        static FieldInfo currentMeter;

        Harmony harmony;
        Timer keepalive;

        void Awake()
        {
            log = Logger;
            // The feed file: the config's path if set, else under the host's home. Proton
            // passes no HOME into the game (found 2026-10-08, first launch), but Wine sets
            // WINEHOMEDIR as \??\unix\home\NAME, which is drive Z: with that prefix off.
            var cfg = Config.Bind("Feed", "Path", "",
                "The feed file to write; empty derives .local/state/g13map/health under the home directory.");
            string home = Environment.GetEnvironmentVariable("HOME");
            string wineHome = Environment.GetEnvironmentVariable("WINEHOMEDIR");
            if (!string.IsNullOrEmpty(cfg.Value))
                path = cfg.Value;
            else if (!string.IsNullOrEmpty(home) && Path.DirectorySeparatorChar == '/')
                path = Path.Combine(home, ".local/state/g13map/health");
            else if (!string.IsNullOrEmpty(home))
                path = "Z:" + home.Replace('/', '\\') + "\\.local\\state\\g13map\\health";
            else if (!string.IsNullOrEmpty(wineHome) && wineHome.StartsWith("\\??\\unix"))
                path = "Z:" + wineHome.Substring(8) + "\\.local\\state\\g13map\\health";
            else
            {
                var names = new StringBuilder();
                foreach (System.Collections.DictionaryEntry e in Environment.GetEnvironmentVariables())
                    names.Append(e.Key).Append(' ');
                Logger.LogWarning("no HOME or WINEHOMEDIR: set [Feed] Path in the config. Environment: " + names);
                path = null;
                return;
            }
            try
            {
                Directory.CreateDirectory(Path.GetDirectoryName(path));
            }
            catch (Exception e)
            {
                Logger.LogWarning("cannot make the directory of " + path + ": " + e.Message);
                path = null;
                return;
            }
            tmp = path + ".new";
            var flags = BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public;
            rankIndex = typeof(StyleHUD).GetField("_rankIndex", flags);
            currentMeter = typeof(StyleHUD).GetField("currentMeter", flags);
            if (rankIndex == null || currentMeter == null)
                Logger.LogWarning("StyleHUD has no _rankIndex/currentMeter: no style on the panel");

            // Ride the player's own frame.
            MethodInfo target = null;
            foreach (var name in new[] { "Update", "LateUpdate", "FixedUpdate" })
            {
                target = AccessTools.Method(typeof(NewMovement), name);
                if (target != null) break;
            }
            if (target == null)
            {
                Logger.LogWarning("NewMovement has no Update to ride: no feed");
                path = null;
                return;
            }
            harmony = new Harmony("g13pad.health");
            harmony.Patch(target, postfix: new HarmonyMethod(typeof(Plugin), nameof(AfterPlayer)));
            keepalive = new Timer(Keepalive, null, 1000, 1000);
            Logger.LogInfo("feeding " + path + " after NewMovement." + target.Name);
        }

        void OnDisable()
        {
            if (log != null) log.LogInfo("component disabled (the patch and the timer carry on)");
        }

        void OnDestroy()
        {
            if (log != null) log.LogInfo("component destroyed (the patch and the timer carry on)");
        }

        void OnApplicationQuit()
        {
            if (path == null) return;
            if (keepalive != null) keepalive.Dispose();
            lock (gate)
            {
                try { File.Delete(path); } catch (Exception) { }
            }
        }

        // The game's thread, once a frame while a player exists.
        static void AfterPlayer(NewMovement __instance)
        {
            int now = Environment.TickCount;
            if (now - composeAt < PeriodMs && composeAt != 0) return;
            composeAt = now;
            string line;
            try
            {
                line = Compose(__instance);
            }
            catch (Exception e)
            {
                if (e.Message != lastError || ticks < 3)
                {
                    lastError = e.Message;
                    log.LogWarning("reading the game: " + e);
                }
                line = "wait ttl 3";
            }
            if (ticks < 3)
            {
                ticks++;
                log.LogInfo("tick " + ticks + ": " + line);
            }
            lock (gate)
            {
                pending = line;
                pendingAt = now;
            }
            Write(line, false);
        }

        // The timer thread, once a second: the last line again so the ttl holds, or
        // `wait` when no player has been seen for a while (the menu).
        static void Keepalive(object _)
        {
            string line;
            lock (gate)
            {
                line = Environment.TickCount - pendingAt > 1500 ? "wait ttl 3" : pending;
            }
            Write(line, true);
        }

        static string Compose(NewMovement nm)
        {
            var sb = new StringBuilder();
            int hp = nm.dead ? 0 : Mathf.Max(0, nm.hp);
            sb.Append(hp).Append("/100");
            if (nm.antiHp > 0.5f)
                sb.Append(" cap ").Append(Mathf.Clamp(Mathf.RoundToInt(100f - nm.antiHp), 0, 100));
            var sh = StyleHUD.Instance;
            if (sh != null && rankIndex != null && currentMeter != null)
            {
                int r = Mathf.Clamp((int)rankIndex.GetValue(sh), 0, Ranks.Length - 1);
                sb.Append(" rank ").Append(Ranks[r]);
                float meter = (float)currentMeter.GetValue(sh);
                int max = (sh.ranks != null && r < sh.ranks.Count) ? sh.ranks[r].maxMeter : 0;
                if (max > 0)
                    sb.Append(" style ").Append(Mathf.Clamp(Mathf.RoundToInt(meter / max * 100f), 0, 100));
            }
            var sm = StatsManager.Instance;
            if (sm != null)
                sb.Append(" time ").Append(sm.seconds.ToString("F1", CultureInfo.InvariantCulture));
            sb.Append(" dash ").Append((nm.boostCharge / 100f).ToString("F1", CultureInfo.InvariantCulture));
            var wc = WeaponCharges.Instance;
            if (wc != null)
                sb.Append(" rail ").Append(Mathf.Clamp(Mathf.RoundToInt(wc.raicharge / 5f * 100f), 0, 100));
            sb.Append(" ttl 3");
            return sb.ToString();
        }

        // On change, and once a second regardless (`always`), so the ttl stays alive.
        static void Write(string line, bool always)
        {
            lock (gate)
            {
                int now = Environment.TickCount;
                if (line == lastLine && now - lastWrite < 1000) return;
                if (!always && line == lastLine) return;
                try
                {
                    File.WriteAllText(tmp, line + "\n");
                    if (File.Exists(path)) File.Replace(tmp, path, null);
                    else File.Move(tmp, path);
                    lastLine = line;
                    lastWrite = now;
                }
                catch (Exception e)
                {
                    if (e.Message != lastError)
                    {
                        lastError = e.Message;
                        log.LogWarning("writing the feed to " + tmp + ": " + e);
                    }
                }
            }
        }
    }
}
