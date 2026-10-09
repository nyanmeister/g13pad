// SPDX-License-Identifier: GPL-3.0-or-later
// .NET Framework 4 compatible: vanilla Windows Terraria uses the CLR, not tML's .NET 8.
using System;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

namespace G13TerrariaVanilla
{
    public static class Observer
    {
        private static readonly Stopwatch Clock = Stopwatch.StartNew();
        private static readonly object Gate = new object();
        private static volatile string pending = "wait ttl 3";
        private static FieldInfo menu, index, players, active, dead, life, lifeMax, mana, manaMax, defense, breath, breathMax;
        private static Timer timer;
        private static string path, temporary, written;
        private static long sampledAt = -100, writtenAt = -1000, warnedAt = -60000, deathUntil;
        private static bool initialized, disabled, stopped;
        private static readonly bool Windows = Environment.OSVersion.Platform == PlatformID.Win32NT;

        // Injected only at normal returns from Main.Update. Never modify game fields.
        public static void Sample(object game)
        {
            if (disabled) return;
            try {
                if (!initialized) Initialize(game.GetType());
                long now = Clock.ElapsedMilliseconds;
                if (now - sampledAt < 100) return;
                sampledAt = now;
                string line = "wait ttl 3";
                if (!(bool)menu.GetValue(null)) {
                    Array all = (Array)players.GetValue(null);
                    int who = (int)index.GetValue(null);
                    object player = who >= 0 && who < all.Length ? all.GetValue(who) : null;
                    if (player != null && (bool)active.GetValue(player)) {
                        int hp = (bool)dead.GetValue(player) ? 0 : Number(life, player, 0);
                        int maxMp = Number(manaMax, player, 0);
                        line = string.Format(CultureInfo.InvariantCulture,
                            "{0}/{1} mana {2}/{3} defense {4} breath {5}/{6} ttl 3",
                            hp, Number(lifeMax, player, 1), maxMp == 0 ? 0 : Number(mana, player, 0),
                            maxMp, Number(defense, player, 0), Number(breath, player, 0), Number(breathMax, player, 1));
                    }
                }
                if (line.StartsWith("0/", StringComparison.Ordinal) && !pending.StartsWith("0/", StringComparison.Ordinal))
                    deathUntil = now + 500;
                if (now >= deathUntil || line.StartsWith("0/", StringComparison.Ordinal)) pending = line;
            }
            catch (Exception error) {
                disabled = true;
                Stop();
                Log("G13 Terraria handler disabled: " + error);
            }
        }

        private static int Number(FieldInfo field, object player, int minimum)
        { return Math.Max(minimum, (int)field.GetValue(player)); }

        private static FieldInfo Field(Type type, string name, Type expected, bool isStatic)
        {
            FieldInfo field = type.GetField(name, BindingFlags.Public | (isStatic ? BindingFlags.Static : BindingFlags.Instance));
            if (field == null || field.FieldType != expected) throw new InvalidOperationException("Unsupported field " + type.FullName + "." + name);
            return field;
        }

        private static void Initialize(Type main)
        {
            Log("Initializing observer for " + main.FullName);
            Type player = main.Assembly.GetType("Terraria.Player", true);
            menu = Field(main, "gameMenu", typeof(bool), true);
            index = Field(main, "myPlayer", typeof(int), true);
            players = Field(main, "player", player.MakeArrayType(), true);
            active = Field(player, "active", typeof(bool), false);
            dead = Field(player, "dead", typeof(bool), false);
            life = Field(player, "statLife", typeof(int), false);
            lifeMax = Field(player, "statLifeMax2", typeof(int), false);
            mana = Field(player, "statMana", typeof(int), false);
            manaMax = Field(player, "statManaMax2", typeof(int), false);
            defense = Field(player, "statDefense", typeof(int), false);
            breath = Field(player, "breath", typeof(int), false);
            breathMax = Field(player, "breathMax", typeof(int), false);
            path = ResolvePath();
            temporary = path + ".terraria-" + Guid.NewGuid().ToString("N") + ".new";
            AppDomain.CurrentDomain.ProcessExit += delegate { Stop(); };
            initialized = true;
            timer = new Timer(Write, null, 100, 100);
            Log("G13 Terraria handler 0.1.0: " + path);
        }

        private static string ResolvePath()
        {
            string value = Environment.GetEnvironmentVariable("G13MAP_HEALTH_FILE");
            if (!string.IsNullOrEmpty(value)) return Absolute(value);
            value = Environment.GetEnvironmentVariable("XDG_STATE_HOME");
            if (!string.IsNullOrEmpty(value)) return Path.Combine(Absolute(value), "g13map", "health");
            value = Environment.GetEnvironmentVariable("HOME");
            if (string.IsNullOrEmpty(value)) {
                string wine = Environment.GetEnvironmentVariable("WINEHOMEDIR");
                if (!string.IsNullOrEmpty(wine) && wine.StartsWith(@"\??\unix\", StringComparison.Ordinal)) value = "Z:" + wine.Substring(8);
            }
            if (string.IsNullOrEmpty(value)) throw new InvalidOperationException("Set G13MAP_HEALTH_FILE; host home was not found.");
            return Path.Combine(Absolute(value), ".local", "state", "g13map", "health");
        }

        private static string Absolute(string value)
        {
            if (Windows && value.StartsWith("/", StringComparison.Ordinal)) value = "Z:" + value.Replace('/', '\\');
            bool absolute = Windows ? value.Length >= 3 && char.IsLetter(value[0]) && value[1] == ':' && (value[2] == '\\' || value[2] == '/') : value.StartsWith("/", StringComparison.Ordinal);
            if (!absolute) throw new ArgumentException("Health feed path must be absolute.");
            value = Path.GetFullPath(value);
            if (string.IsNullOrEmpty(Path.GetFileName(value))) throw new ArgumentException("Health feed path must name a file.");
            return value;
        }

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool MoveFileEx(string source, string destination, uint flags);
        [DllImport("libc", EntryPoint = "rename", SetLastError = true)]
        private static extern int Rename(string source, string destination);

        private static void Write(object unused)
        {
            lock (Gate) {
                if (stopped) return;
                long now = Clock.ElapsedMilliseconds;
                string line = pending;
                if (line == written && now - writtenAt < 1000) return;
                try {
                    Directory.CreateDirectory(Path.GetDirectoryName(path));
                    using (var stream = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None))
                    using (var writer = new StreamWriter(stream, new UTF8Encoding(false))) writer.WriteLine(line);
                    bool moved = Windows ? MoveFileEx(temporary, path, 1) : Rename(temporary, path) == 0;
                    if (!moved) throw new IOException("Atomic feed rename failed: " + Marshal.GetLastWin32Error());
                    written = line;
                    writtenAt = now;
                }
                catch (Exception error) {
                    Cleanup();
                    if (now - warnedAt >= 60000) {
                        warnedAt = now;
                        Log("G13 Terraria feed: " + error.Message + " (will retry)");
                    }
                }
            }
        }

        private static void Cleanup()
        { try { if (temporary != null) File.Delete(temporary); } catch (IOException) { } catch (UnauthorizedAccessException) { } catch (System.Security.SecurityException) { } }
        private static void Stop()
        {
            lock (Gate) {
                stopped = true;
                if (timer != null) timer.Dispose();
                Cleanup();
            }
        }

        private static readonly object LogGate = new object();
        private static void Log(string message)
        {
            Console.Error.WriteLine(message);
            // Windows GUI executables often have no CLR console handles. Keep a
            // diagnostic beside the copy, using a worker so startup IO cannot stall Update.
            ThreadPool.QueueUserWorkItem(delegate {
                try {
                    lock (LogGate) File.AppendAllText(Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "G13Terraria.log"),
                        DateTime.UtcNow.ToString("o", CultureInfo.InvariantCulture) + " " + message + Environment.NewLine);
                } catch (IOException) { } catch (UnauthorizedAccessException) { } catch (System.Security.SecurityException) { }
            });
        }
    }
}
