// SPDX-License-Identifier: GPL-3.0-or-later
using System;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Threading;

namespace G13TerrariaHealth;

internal sealed class HealthFeed : IDisposable
{
    private readonly object gate = new();
    private readonly Timer timer;
    private readonly Action<string> warn;
    private readonly string temporary;
    private readonly Stopwatch clock = Stopwatch.StartNew();
    private volatile string pending = "wait ttl 3";
    private string written;
    private long writtenAt = -1000;
    private long warnedAt = -60000;
    private long deathUntil;
    private volatile bool stopped;
    public string Path { get; }

    public HealthFeed(string path, Action<string> warning)
    {
        Path = path;
        warn = warning;
        temporary = path + ".terraria-" + Guid.NewGuid().ToString("N") + ".new";
        timer = new Timer(Write, null, 0, 100);
    }

    public void Sample(string line)
    {
        // Only the game's thread calls Sample. A slow disk must not hold it up.
        if (stopped) return;
        if (line.StartsWith("0/", StringComparison.Ordinal) &&
            !pending.StartsWith("0/", StringComparison.Ordinal))
            deathUntil = clock.ElapsedMilliseconds + 500;
        if (clock.ElapsedMilliseconds < deathUntil &&
            !line.StartsWith("0/", StringComparison.Ordinal)) return;
        pending = line;
    }

    private void Write(object unused)
    {
        lock (gate) {
            if (stopped) return;
            long now = clock.ElapsedMilliseconds;
            string line = pending;
            if (line == written && now - writtenAt < 1000) return;
            try {
                Directory.CreateDirectory(System.IO.Path.GetDirectoryName(Path));
                using (var stream = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None))
                using (var writer = new StreamWriter(stream, new UTF8Encoding(false)))
                    writer.Write(line + "\n");
                File.Move(temporary, Path, true);
                written = line;
                writtenAt = now;
            }
            catch (Exception error) when (error is IOException || error is UnauthorizedAccessException || error is System.Security.SecurityException) {
                try { File.Delete(temporary); }
                catch (Exception cleanup) when (cleanup is IOException || cleanup is UnauthorizedAccessException || cleanup is System.Security.SecurityException) { }
                if (now - warnedAt >= 60000) {
                    warnedAt = now;
                    warn($"Terraria health feed: {error.Message} (will retry)");
                }
            }
        }
    }

    public void Dispose()
    {
        // Serialize shutdown with callbacks; never remove another game's feed.
        lock (gate) {
            stopped = true;
            timer.Dispose();
            try { File.Delete(temporary); } catch (IOException) { }
            catch (UnauthorizedAccessException) { }
            catch (System.Security.SecurityException) { }
        }
        // The last line expires after three seconds, including abnormal exits.
    }

    internal static string ResolvePath()
    {
        string configured = Environment.GetEnvironmentVariable("G13MAP_HEALTH_FILE");
        if (!string.IsNullOrEmpty(configured)) return Absolute(configured);

        string state = Environment.GetEnvironmentVariable("XDG_STATE_HOME");
        if (!string.IsNullOrEmpty(state))
            return System.IO.Path.Combine(Absolute(state), "g13map", "health");

        string home = Environment.GetEnvironmentVariable("HOME");
        if (string.IsNullOrEmpty(home)) {
            string wine = Environment.GetEnvironmentVariable("WINEHOMEDIR");
            if (!string.IsNullOrEmpty(wine) && wine.StartsWith(@"\??\unix\", StringComparison.Ordinal))
                home = "Z:" + wine.Substring(8);
        }
        if (string.IsNullOrEmpty(home) && !OperatingSystem.IsWindows())
            home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        if (string.IsNullOrEmpty(home))
            throw new InvalidOperationException("Set G13MAP_HEALTH_FILE to the host feed path; no host home was found.");
        return System.IO.Path.Combine(Absolute(home), ".local", "state", "g13map", "health");
    }

    private static string Absolute(string path)
    {
        if (OperatingSystem.IsWindows() && path.StartsWith("/", StringComparison.Ordinal))
            path = "Z:" + path.Replace('/', '\\');
        if (!System.IO.Path.IsPathFullyQualified(path))
            throw new ArgumentException("Health feed paths must be absolute.");
        path = System.IO.Path.GetFullPath(path);
        if (string.IsNullOrEmpty(System.IO.Path.GetFileName(path)))
            throw new ArgumentException("Health feed paths must name a file.");
        return path;
    }
}
