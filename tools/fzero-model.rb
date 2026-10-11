#!/usr/bin/env ruby
# SPDX-License-Identifier: GPL-3.0-or-later
# Exploratory offline model. Never reads game RAM or changes LCD calibration.
require 'json'
if ARGV.first == '--version'
  puts 'g13-fzero-model 1.0.0'
  exit
end
abort 'usage: fzero-model.rb INPUT_JSON' unless ARGV.size == 1
raise 'input too large' if File.size(ARGV[0]) > 1024 * 1024
input = JSON.parse(File.read(ARGV[0]))

def constants(row)
  m, a, s, d = %w[weight acceleration top_coefficient drag].map { |k| Float(row.fetch(k)) }
  raise 'invalid machine coefficients' unless [m, a, s, d].all?(&:finite?) && m > 0 && a > 0 && s >= 0 && d >= 0
  q = 40.0 * a / 348.0
  k = (q - d) * (36.0 + 40.0 * s) / (4.0 * a * (a + 0.6))
  raise 'outside candidate domain' unless k.finite? && k > 0
  [m, k]
end

def inferred_resistance(row)
  _, k = constants(row)
  v = Float(row.fetch('measured_kmh')) / 216.0
  raise 'invalid measured speed' unless v.finite? && v > 0
  delta = 2.0 * k / (Math.sqrt(v * v + 4.0 * k) + v)
  delta / (v * v)
end

def fit(rows)
  raise 'too few training setups' unless rows.size >= 3
  points = rows.map { |r| [1.0 / constants(r)[0], inferred_resistance(r)] }
  xmean = points.sum(&:first) / points.size
  ymean = points.sum(&:last) / points.size
  variance = points.sum { |x, _| (x - xmean)**2 }
  raise 'need distinct masses' unless variance > 0
  b = points.sum { |x, y| (x - xmean) * (y - ymean) } / variance
  [ymean - b * xmean, b]
end

def predict(row, coefficients)
  mass, k = constants(row)
  c = coefficients[0] + coefficients[1] / mass
  raise 'invalid fitted resistance' unless c.finite? && c > 0
  lo = 0.0; hi = 100.0
  upper_delta = c * hi * hi
  raise 'candidate root outside bounded solver' unless upper_delta * (hi + upper_delta) >= k
  80.times do
    v = (lo + hi) / 2.0
    delta = c * v * v
    if delta * (v + delta) > k
      hi = v
    else
      lo = v
    end
  end
  (lo + hi) / 2.0 * 216.0
end

def residuals(rows, coefficients)
  rows.map do |r|
    prediction = predict(r, coefficients)
    {label: r.fetch('label'), predicted_kmh: prediction, measured_kmh: r.fetch('measured_kmh'),
     error_kmh: prediction - r.fetch('measured_kmh'),
     error_percent: 100.0 * (prediction / r.fetch('measured_kmh') - 1.0)}
  end
end

training = input.fetch('training')
validation = input.fetch('validation', [])
raise 'bounded arrays required' unless training.is_a?(Array) && validation.is_a?(Array) &&
  (3..64).cover?(training.size) && validation.size <= 64
coefficients = fit(training)
groups = training.map { |r| r.fetch('group') }.uniq
cross_validation = groups.map do |group|
  other = training.reject { |r| r.fetch('group') == group }
  {held_out_group: group, residuals: residuals(training.select { |r| r.fetch('group') == group }, fit(other))}
end
target = input.fetch('target')
raise 'target must not include an observed speed' if target.key?('measured_kmh')
puts JSON.pretty_generate({
  status: 'Exploratory candidate; propulsion constants from game code, two resistance coefficients fit to measurements. Not a validated universal model.',
  equations: ['q = 40*A/348', 'K = (q-drag)*(36+40*S)/(4*A*(A+0.6))',
              'c = c0 + c1/mass', 'delta = c*v*v', 'delta*(v+delta) = K', 'kmh = 216*v'],
  coefficients: coefficients, training: residuals(training, coefficients),
  validation: residuals(validation, coefficients), group_cross_validation: cross_validation,
  target: target, prediction_kmh: predict(target, coefficients)
})
