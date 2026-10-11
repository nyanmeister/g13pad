#!/usr/bin/env ruby
# SPDX-License-Identifier: GPL-3.0-or-later
# Offline summary for v2 captures. Keeps horizontal track orientations separate.
require 'json'
if ARGV.first == '--version'
  puts 'g13-fzero-analyze 1.1.0'
  exit
end
abort 'usage: fzero-analyze.rb CAPTURE_JSON [CAPTURE_JSON ...]' unless (1..16).cover?(ARGV.size)

def eligible?(a)
  a.fetch('flags')[0] & 0x10 != 0 && a['flags'][0] & 9 == 0 && a['flags'][1] & 0x30 == 0 &&
    a.fetch('boost') == 0 && a.fetch('boost_effect').abs < 0.001 && a.fetch('boost_camera').abs < 0.001 &&
    a.fetch('energy_boost', 0) == 0 && a.fetch('boost_delay', 0) == 0 &&
    a.fetch('throttle') >= 0.99 && a.fetch('brake').abs < 0.01 && a.fetch('steer').abs < 0.01 &&
    a.fetch('strafe').abs < 0.01 && a.fetch('pitch').abs < 0.01 && a.fetch('grip') == 0 &&
    a.fetch('air_time') == 0 && a.fetch('collision_timer') == 0 && a.fetch('terrain') == 0
end

def orientation(a)
  track = a.fetch('track')
  return nil unless track[0].abs < 0.001 && track[2].abs < 0.001
  return 1 if (track[1] - 1).abs < 0.001
  return -1 if (track[1] + 1).abs < 0.001
  nil
end

reports = ARGV.map do |path|
  raise 'capture too large' if File.size(path) > 8 * 1024 * 1024
  samples = JSON.parse(File.read(path))
  raise 'expected bounded v2 sample array' unless samples.is_a?(Array) && samples.size <= 100_000
  # Paused games repeat the same frame. Such rows add no physical evidence.
  unique = []; duplicates = 0
  samples.each do |a|
    if unique.last && unique.last.fetch('key') == a.fetch('key') && unique.last.fetch('frame') == a.fetch('frame')
      duplicates += 1
    else
      unique << a
    end
  end
  report = {capture: path, samples: samples.size, duplicate_frames: duplicates,
            setups: samples.map { |a| a.fetch('key') }.uniq}
  [1, -1].each do |sign|
    groups = []; group = []
    unique.each do |a|
      if eligible?(a) && orientation(a) == sign
        if group.last && (group.last['key'] != a['key'] || a['frame'] <= group.last['frame'] ||
                          a['frame'] - group.last['frame'] > 12 || group.last['wall_hits'] != a['wall_hits'])
          groups << group
          group = []
        end
        group << a
      else
        groups << group unless group.empty?
        group = []
      end
    end
    groups << group unless group.empty?
    report[sign == 1 ? :upright : :inverted] = {
      eligible_samples: groups.sum(&:size),
      segments: groups.select { |g| g.size >= 3 }.map do |g|
        tail = g.last(8)
        seconds = (tail.last['frame'] - tail.first['frame']) / 60.0
        {key: g.first['key'], n: g.size, t: [g.first['t'], g.last['t']],
         frame: [g.first['frame'], g.last['frame']],
         game_seconds: (g.last['frame'] - g.first['frame']) / 60.0,
         speed: [g.first['speed'], g.last['speed']], range: g.map { |a| a['speed'] }.minmax,
         tail_range: tail.map { |a| a['speed'] }.minmax,
         tail_slope_kmh_per_game_second: seconds > 0 ? (tail.last['speed'] - tail.first['speed']) / seconds : nil,
         wall_hits: g.map { |a| a['wall_hits'] }.uniq,
         energy: g.map { |a| a['energy'] }.minmax}
      end
    }
  end
  report
end
puts JSON.pretty_generate(reports)
