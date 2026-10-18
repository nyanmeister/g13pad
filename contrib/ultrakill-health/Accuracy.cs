// SPDX-License-Identifier: GPL-3.0-or-later
using System;
using System.Collections.Generic;

namespace G13Health
{
    // One ledger per level. An old projectile keeps its old ledger, so a late hit
    // after a scene change cannot change the new level's counters.
    internal sealed class Accuracy
    {
        internal ulong Shots { get; private set; }
        internal ulong Hits { get; private set; }
        internal bool Available = true;

        internal Cause Fire()
        {
            Shots++;
            return new Cause(new[] { new Shot(this) });
        }

        internal sealed class Shot
        {
            readonly Accuracy owner;
            internal bool Hit;
            internal Shot(Accuracy owner) { this.owner = owner; }
            internal void Connect()
            {
                if (Hit) return;
                Hit = true;
                owner.Hits++;
            }
        }

        internal sealed class Cause
        {
            readonly Shot[] shots;
            internal Cause(Shot[] shots) { this.shots = shots; }
            internal void Connect() { foreach (var shot in shots) shot.Connect(); }

            // A projectile and the attack detonating it both contribute. Duplicate
            // roots (pellets, pierced enemies, split coins) still get only one hit.
            internal static Cause Merge(Cause a, Cause b)
            {
                if (a == null) return b;
                if (b == null || ReferenceEquals(a, b)) return a;
                var roots = new List<Shot>();
                foreach (var shot in a.shots) if (!shot.Hit) roots.Add(shot);
                foreach (var shot in b.shots)
                    if (!shot.Hit && !roots.Contains(shot)) roots.Add(shot);
                return roots.Count == 0 ? null : new Cause(roots.ToArray());
            }
        }
    }
}
