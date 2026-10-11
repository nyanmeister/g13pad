#!/usr/bin/env ruby
# SPDX-License-Identifier: GPL-3.0-or-later
# Bounded, read-only diagnostic captures; never writes emulator RAM or calibration.
require 'json'
if ARGV.first == '--version'
  puts 'g13-fzero-sample 2.1.0'
  exit
end
abort 'usage: fzero-sample.rb PID OUTPUT_JSON [SECONDS<=45]' unless (2..3).cover?(ARGV.size)
pid = Integer(ARGV[0]); output = ARGV[1]; duration = Float(ARGV.fetch(2, '30'))
raise 'duration bound' unless duration.finite? && duration > 0 && duration <= 45
start_id = -> { File.read("/proc/#{pid}/stat").split(') ', 2).last.split.fetch(19) }
identity = start_id.call
fd = Dir.children("/proc/#{pid}/fd").find do |entry|
  File.readlink("/proc/#{pid}/fd/#{entry}").match?(%r{\A/dev/shm/dolphin-emu\.#{pid}(?: \(deleted\))?\z}) rescue false
end
raise 'no Dolphin shared RAM' unless fd
path = "/proc/#{pid}/fd/#{fd}"
file = File.open(path, 'rb')
inode = [file.stat.dev, file.stat.ino]
read = lambda do |size, at|
  raise 'RAM bounds' unless at >= 0 && at + size <= 0x1800000
  bytes = file.pread(size, at)
  raise 'short read' unless bytes.bytesize == size
  bytes
end
header = "GFZE01\0\0"
raise 'wrong disc/revision' unless read.call(8, 0) == header
started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
samples = []; skipped = 0
begin
  loop do
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - started
    break if elapsed >= duration
    break unless start_id.call == identity && [File.stat(path).dev, File.stat(path).ino] == inode
    break unless read.call(8, 0) == header
    root = read.call(4, 0x30c8).unpack1('N')
    raise 'invalid root' unless (0x80000000...0x81800000).cover?(root) && root % 4 == 0
    r = root - 0x80000000
    flags = read.call(2, r + 0x1bdc71).bytes
    pointer_at = r + 0x227878
    pointer = read.call(4, pointer_at).unpack1('N')
    if pointer % 4 == 0 && pointer >= 0x80000000 && pointer + 0x620 <= 0x81800000
      b = read.call(0x620, pointer - 0x80000000)
      f = ->(at) { b[at, 4].unpack1('g') }
      floats = [8, 0x220, 0x224, 0x22c, 0x23c, 0x17c, 0x184, 0x228, 0x18c,
                0x200, 0x204, 0x1fc, 0x1f8, 0x1f4, 0xb8, 0xbc, 0xc0, 0x1bc, 0x1c0, 0x1c4]
      consistent = read.call(4, 0x30c8).unpack1('N') == root &&
                   read.call(4, pointer_at).unpack1('N') == pointer &&
                   read.call(2, r + 0x1bdc71).bytes == flags
      if consistent && b[4, 2].unpack1('n') == 0 && floats.all? { |at| f.call(at).finite? }
        machine = b[6, 2].unpack1('n')
        key = [machine, *[8, 0x220, 0x22c, 0x23c].map { |at| '%08x' % b[at, 4].unpack1('N') }].join('-')
        samples << {
          t: elapsed.round(4), flags: flags, machine: machine,
          name: b[0x3c, 32].split("\0")[0].force_encoding('UTF-8').scrub, key: key,
          weight: f.call(8), acceleration: f.call(0x220), top_coefficient: f.call(0x22c),
          drag: f.call(0x23c), base_speed: f.call(0x224), speed: f.call(0x17c), energy: f.call(0x184),
          boost: b.getbyte(0x18a), energy_boost: b.getbyte(0x18b),
          boost_effect: f.call(0x228), boost_camera: f.call(0x18c),
          boost_delay: b[0x4c6, 2].unpack1('n'),
          throttle: f.call(0x200), brake: f.call(0x204), steer: f.call(0x1fc), strafe: f.call(0x1f8), pitch: f.call(0x1f4),
          grip: b.getbyte(0x247), terrain: b.getbyte(0x218), contact_flags: b.getbyte(0x61c),
          frame: b[0x47c, 4].unpack1('N'), air_time: b.getbyte(0x483), wall_hits: b.getbyte(0x213),
          collision_timer: b[0x4c4, 2].unpack1('n'),
          momentum: [0xb8, 0xbc, 0xc0].map { |at| f.call(at) },
          track: [0x1bc, 0x1c0, 0x1c4].map { |at| f.call(at) }
        }
      else
        skipped += 1
      end
    end
    sleep 0.1
  end
ensure
  file.close
  temp = "#{output}.#{Process.pid}.tmp"
  File.write(temp, JSON.pretty_generate(samples))
  File.rename(temp, output)
end
puts JSON.generate(samples: samples.size, skipped: skipped,
                   setups: samples.map { |s| [s[:machine], s[:name], s[:key]] }.uniq)
