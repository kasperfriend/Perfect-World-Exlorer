#!/usr/bin/env python3
"""Cross-check the block surface / LOD rules in src/terrain_lod.rs.

Python mirror of `block_surface`, `edge_case`, `window` and `LodConfig` from the
Rust module, with a rasterised coverage test. Used because this checkout's
sandbox has no Rust toolchain: the same checks exist as `#[cfg(test)]` unit
tests in `src/terrain_lod.rs`; this script executes them here.

Usage: tools/verify_terrain_lod.py
Exit code 0 means every check passed.
"""
import math

CELLS = 32
MAX_LEVEL = 3
HALF_DIAGONAL = 0.70710678


def step(level):
    return 1 << (MAX_LEVEL - min(max(level, 1), MAX_LEVEL))


def interior_cells(level):
    s = step(level)
    return (CELLS - 2 * s) // s


def edge_case(own, neighbour):
    if neighbour is None or neighbour == 0 or neighbour >= own:
        return 0
    return min(own - neighbour, 2)


def edge_stride(level, case):
    return step(level) << case


def block_surface(level, cases):
    level = min(max(level, 1), MAX_LEVEL)
    s = step(level)
    u = CELLS // s
    grid = []
    index_of = {}
    indices = []

    def v(i, j):
        key = (i, j)
        idx = index_of.get(key)
        if idx is None:
            idx = len(grid)
            index_of[key] = idx
            grid.append((i * s, j * s))
        return idx

    def tri(a, b, c):
        if a != b and b != c and a != c:
            indices.extend((a, b, c))

    q = interior_cells(level)
    assert q == u - 2, (q, u)
    for j in range(1, u - 1):
        for i in range(1, u - 1):
            a = v(i, j)
            c = v(i + 1, j)
            d = v(i, j + 1)
            e = v(i + 1, j + 1)
            tri(a, c, d)
            tri(c, e, d)
    for side in range(4):
        m = 1 << min(cases[side], 2)
        outer_line, inner_line = [(0, 1), (u, u - 1), (0, 1), (u, u - 1)][side]
        horizontal = side < 2

        def outer(t):
            return v(t, outer_line) if horizontal else v(outer_line, t)

        def inner(t):
            return v(t, inner_line) if horizontal else v(inner_line, t)

        a = 0
        while a < u:
            e = min(a + m, u)
            oa = outer(a)
            chain = [inner(t) for t in range(a, e + 1) if 1 <= t <= u - 1]
            oe = outer(e)
            if not chain:
                tri(oa, oe, oa)
            else:
                for w in range(len(chain) - 1):
                    tri(oa, chain[w], chain[w + 1])
                tri(oa, chain[-1], oe)
            a = e
    return grid, indices


def triangles(grid, indices, origin=(0.0, 0.0)):
    out = []
    for k in range(0, len(indices), 3):
        out.append(
            tuple(
                (origin[0] + grid[indices[k + o]][0], origin[1] + grid[indices[k + o]][1])
                for o in range(3)
            )
        )
    return out


def point_in_triangle(p, t):
    def sign(a, b, c):
        return (a[0] - c[0]) * (b[1] - c[1]) - (b[0] - c[0]) * (a[1] - c[1])

    d1 = sign(p, t[0], t[1])
    d2 = sign(p, t[1], t[2])
    d3 = sign(p, t[2], t[0])
    neg = d1 < 0 or d2 < 0 or d3 < 0
    pos = d1 > 0 or d2 > 0 or d3 > 0
    return not (neg and pos)


def coverage(tris, lo, hi, step_size):
    """Count how many triangles cover each lattice sample (bbox-accelerated)."""
    nx = int((hi - lo) / step_size) + 2
    counts = [0] * (nx * nx)
    xs = [lo + step_size * 0.37 + k * step_size for k in range(nx)]
    ys = [lo + step_size * 0.61 + k * step_size for k in range(nx)]
    for t in tris:
        xmin = min(p[0] for p in t)
        xmax = max(p[0] for p in t)
        ymin = min(p[1] for p in t)
        ymax = max(p[1] for p in t)
        i0 = max(0, int((xmin - lo) / step_size) - 1)
        i1 = min(nx - 1, int((xmax - lo) / step_size) + 2)
        j0 = max(0, int((ymin - lo) / step_size) - 1)
        j1 = min(nx - 1, int((ymax - lo) / step_size) + 2)
        for j in range(j0, j1 + 1):
            y = ys[j]
            if y < ymin or y > ymax:
                continue
            row = j * nx
            for i in range(i0, i1 + 1):
                x = xs[i]
                if x < xmin or x > xmax:
                    continue
                if x >= hi or y >= hi:
                    continue
                if point_in_triangle((x, y), t):
                    counts[row + i] += 1
    inside = 0
    bad = []
    for j in range(nx):
        y = ys[j]
        if y >= hi:
            break
        for i in range(nx):
            x = xs[i]
            if x >= hi:
                break
            inside += 1
            if counts[j * nx + i] != 1:
                bad.append((round(x, 3), round(y, 3), counts[j * nx + i]))
    return inside, bad


def window(cell, dims, rng, block_size):
    r = max(0, math.ceil(rng / block_size))
    width = 2 * r + 1
    centre = (math.floor(cell[0]), math.floor(cell[1]))

    def axis(c, dim):
        lo, hi = c - r, c + r + 1
        if lo < 0:
            lo, hi = 0, min(width, dim)
        if hi > dim:
            hi, lo = dim, max(dim - width, 0)
        return max(lo, 0), max(hi, 0)

    x0, x1 = axis(centre[0], dims[0])
    y0, y1 = axis(centre[1], dims[1])
    return (x0, y0, x1, y1)


def level_at(cam, centre, block=64.0, near=1.5, far=3.0):
    dx = centre[0] - cam[0]
    dz = centre[1] - cam[1]
    d = math.hypot(dx, dz) - block * HALF_DIAGONAL
    if d < near * block:
        return 3
    if d < far * block:
        return 2
    return 1


def main():
    failures = 0

    # 1. window
    assert window((50.0, 40.5), (128, 176), 420.0, 64.0) == (43, 33, 58, 48)
    assert window((1.0, 2.0), (128, 176), 420.0, 64.0) == (0, 0, 15, 15)
    assert window((127.0, 175.0), (128, 176), 420.0, 64.0) == (113, 161, 128, 176)
    assert window((0.0, 0.0), (4, 4), 420.0, 64.0) == (0, 0, 4, 4)
    print("window ...................... ok (r=ceil(range/block), 2r+1, shifted at edges)")

    # 2. levels
    assert level_at((32.0, 32.0), (32.0, 32.0)) == 3
    edge = 1.5 * 64 + 64 * HALF_DIAGONAL
    assert level_at((0.0, 0.0), (edge - 0.01, 0.0)) == 3
    assert level_at((0.0, 0.0), (edge + 0.01, 0.0)) == 2
    edge = 3.0 * 64 + 64 * HALF_DIAGONAL
    assert level_at((0.0, 0.0), (edge - 0.01, 0.0)) == 2
    assert level_at((0.0, 0.0), (edge + 0.01, 0.0)) == 1
    print("levels ...................... ok (two thresholds, -half diagonal, no hysteresis)")

    # 3. seam strides agree from both sides
    for own in (1, 2, 3):
        for other in (1, 2, 3):
            if own == other:
                continue
            a = edge_stride(own, edge_case(own, other))
            b = edge_stride(other, edge_case(other, own))
            assert a == b, (own, other, a, b)
    assert edge_case(3, None) == 0 and edge_case(3, 0) == 0
    print("seam stride ................. ok (max(own, neighbour) step, missing -> own)")

    # 4. per-block exact coverage
    combos = [
        (0, 0, 0, 0),
        (1, 0, 0, 0),
        (0, 1, 0, 0),
        (0, 0, 1, 0),
        (0, 0, 0, 1),
        (1, 1, 1, 1),
        (2, 2, 2, 2),
        (2, 0, 2, 0),
        (0, 2, 0, 2),
        (1, 2, 1, 2),
    ]
    for level in (1, 2, 3):
        cell = float(step(level))
        for cases in combos:
            grid, indices = block_surface(level, cases)
            tris = triangles(grid, indices)
            n, bad = coverage(tris, 0.0, float(CELLS), cell * 0.25)
            status = "ok" if not bad else "FAIL"
            if bad:
                failures += 1
                print(
                    f"  block level {level} cases {cases}: {len(bad)}/{n} bad samples, "
                    f"first {bad[:3]}"
                )
            print(
                f"block level {level} cases {cases}: {len(tris):5d} tris, "
                f"{n:5d} samples, {len(bad):3d} bad  {status}"
            )

    # 5. mixed-level 2x2 neighbourhood
    levels = [[3, 2], [1, 3]]
    tris = []

    def at(x, y):
        if 0 <= x < 2 and 0 <= y < 2:
            return levels[y][x]
        return None

    for y in range(2):
        for x in range(2):
            own = levels[y][x]
            cases = tuple(
                edge_case(own, at(*nb))
                for nb in ((x, y - 1), (x, y + 1), (x - 1, y), (x + 1, y))
            )
            grid, indices = block_surface(own, cases)
            tris.extend(triangles(grid, indices, (x * CELLS, y * CELLS)))
    n, bad = coverage(tris, 0.0, float(2 * CELLS), 0.25)
    print(
        f"2x2 mixed-level field: {len(tris)} tris, {n} samples, {len(bad)} bad "
        f"{'ok' if not bad else 'FAIL'}"
    )
    if bad:
        failures += 1
        print("  first bad samples:", bad[:5])

    # 6. shared edge node sets are identical for every level pair
    for own in (1, 2, 3):
        for other in (1, 2, 3):
            seq_own = sorted(
                y
                for y in range(0, CELLS + 1)
                if y % edge_stride(own, edge_case(own, other)) == 0
            )
            seq_other = sorted(
                y
                for y in range(0, CELLS + 1)
                if y % edge_stride(other, edge_case(other, own)) == 0
            )
            assert seq_own == seq_other, (own, other, seq_own, seq_other)
    print("shared edge nodes ........... ok (identical sets for all level pairs)")

    print("FAILURES:", failures)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
