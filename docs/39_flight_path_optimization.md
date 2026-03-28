# TSP and Vehicle Routing Algorithms for Flight Line Ordering

> **Source document:** 39_Flight-Path-Optimization.docx
>
> **Scope:** Complete extraction — all content is unique. Covers ATSP formulation for aerial survey, nearest-neighbor construction, 2-opt/3-opt/Or-opt local search with wind asymmetry, boustrophedon detection, metaheuristic necessity analysis, and multi-block hierarchical routing.

---

## 1. Why ATSP, Not TSP

Flight line routing is strictly an **Asymmetric Traveling Salesman Problem (ATSP)**, not a symmetric TSP, for two reasons:

**Bidirectionality:** Each flight line can be flown forward (A→B) or reverse (B→A). The exit coordinate determines the transition path to the next line. The solver must optimize both sequence AND direction.

**Wind asymmetry:** Cost(A→B) ≠ Cost(B→A). A headwind leg takes longer than the same geographic track with a tailwind. Turning into a headwind produces a tighter ground track radius (shorter, faster). Turning with a tailwind produces a "blown-out" turn (larger radius, longer recovery). Averaging directional costs `(c_ij + c_ji)/2` produces sub-optimal, fuel-inefficient, or physically infeasible trajectories.

### ATSP Formulation
Node-doubling: each physical flight line L_i creates two mutually exclusive directed nodes (L_i_forward, L_i_reverse) with infinite cross-penalty. ILP with binary decision variables, assignment constraints (exactly one incoming/outgoing arc per node), and DFJ subtour elimination constraints (exponential in n — intractable for real-time without branch-and-cut).

---

## 2. Boustrophedon Detection (Short-Circuit)

**For parallel grids, the serpentine pattern is the proven global optimum.** Any deviation increases non-productive transition distance.

### Detection Algorithm
1. Compute azimuth of all flight lines.
2. If azimuth variance = 0 (all parallel) AND adjacent endpoint distances don't violate minimum turn radius → classify as simple grid.
3. Sort lines orthogonally to their azimuth, assign alternating directions.
4. **O(n log n) deterministic sort — mathematically guaranteed global optimum** for this geometry.

### When Boustrophedon Fails
- **Concave boundaries:** Adjacent lines have vastly different lengths; interior exclusion zones split lines into disjoint segments.
- **Corridor mapping:** Non-parallel polylines with wildly varying transition angles. May require skipping segments and returning.
- **Disjoint multi-polygon blocks:** Simple sorting can't determine inter-block sequence.

When parallelism cannot be guaranteed → fall back to full ATSP heuristics.

---

## 3. Tour Construction: Nearest-Neighbor

### Algorithm
1. Mark all directed flight lines as unvisited.
2. Start from the line closest to the launch point.
3. At each step: evaluate asymmetric transition cost to all unvisited nodes, select the minimum.
4. For each candidate, evaluate BOTH directions and pick the cheaper one.
5. Repeat until all lines visited, close the loop.

**Complexity:** O(n²). Brute-force matrix evaluation is often faster than spatial tree structures because of contiguous memory layout, SIMD vectorization, and avoidance of pointer-chasing/branch-misprediction overhead.

### Quality
- Random distributions: typically 15–25% longer than optimal.
- Worst-case: O(log n) × optimal (adversarial matrices can produce the worst possible tour).
- **Parallel grids:** Near-optimal — the NN organically reconstructs the boustrophedon pattern.
- **Complex boundaries:** Catastrophic "forgotten node" failure — greedy consumption of easy lines leaves isolated nodes requiring massive cross-polygon ferry flights.

### Variants
| Variant | Complexity | Improvement |
|---|---|---|
| Basic NN | O(n²) | Susceptible to forgotten-node penalty |
| Double-Ended NN | O(n²) | Grows from both ends, reduces final jumps |
| Repetitive NN (RNN) | O(n³) | Runs NN from every start node, picks best |

---

## 4. Local Search: 2-Opt

### Mechanic
Delete 2 non-adjacent edges, reconnect the two paths. This **always reverses the intermediate sub-tour**.

### The ATSP Complication
In symmetric TSP: cost delta is O(1) — just compare 4 edge weights (2 new - 2 old). Reversing the sub-tour doesn't change internal costs.

**In ATSP with wind: reversing the sub-tour changes EVERYTHING.** Every flight line in the reversed segment is now flown in the opposite direction (headwind ↔ tailwind). Every turn entry/exit vector is flipped. A single 2-opt move evaluation escalates from O(1) to **O(n)** (must recalculate every edge in the reversed segment).

Full 2-opt pass: O(n²) moves × O(n) evaluation = **O(n³)** — severe bottleneck for large missions.

---

## 5. Local Search: 3-Opt (Direction Preservation)

### Mechanic
Delete 3 edges → 3 disjoint sub-tours → 8 possible reconnections.
- 1 reconstructs the original tour (no change)
- 3 are equivalent to 2-opt moves (require reversal)
- **4 are pure 3-opt moves — some preserve the original direction of all sub-tours**

### Why This Matters for Aerial Routing
Direction-preserving 3-opt moves rearrange sub-tour ORDER (e.g., [A][B][C] → [A][C][B]) without reversing any internal flight directions. Wind-adjusted costs within each sub-tour are unchanged → cost delta evaluation is **O(1)**.

"To cope with this problem [of asymmetric directional costs], Freisleben and Merz (1996) used 3-opt instead of 2-opt to preserve the direction of the path."

**Search space:** O(n³) edge triplets — larger than 2-opt, but O(1) evaluation for direction-preserving moves makes it highly competitive in wind environments.

---

## 6. Or-Opt (Sequence Relocation)

### Mechanic
A restricted subset of 3-opt. Extract a chain of k=1, 2, or 3 consecutive flight lines and insert the chain at a different position in the tour. **Never reverses any sequence** — the chain maintains its original internal order and direction.

### Why Or-Opt Excels for Aerial Routing
- **Rescues "forgotten nodes":** The single most common NN failure. Or-opt extracts the isolated line and inserts it alongside its spatial neighbors. No reversal → no wind cost recalculation.
- **Search space:** O(n²) — same as 2-opt.
- **Evaluation cost:** O(1) per move — no sub-tour recalculation needed.
- **Combines the speed of 2-opt with the direction preservation of 3-opt.**

### Recommended Pipeline for IronTrack
```
NN construction (O(n²)) → Or-opt refinement (O(n²)) → done
```
Sub-millisecond execution for 100-line missions. Results typically within 1–3% of global optimum.

---

## 7. Metaheuristics: When Necessary

### Simulated Annealing (SA)
Accepts worse solutions with probability `exp(-ΔC/T)` at high temperature, transitioning to strict greedy as temperature cools. Escapes local minima that trap 2-opt/Or-opt.

### Genetic Algorithms (GA)
Population-based: crossover blends sub-tours from different solutions, mutation maintains diversity. Computationally expensive; usually hybridized with local search (Memetic Algorithms).

### Sufficiency Analysis for Aerial Survey

**For standard missions (10–200 parallel lines):** NN + Or-opt is sufficient. The structured geometry produces a largely unimodal fitness landscape. Local optima are within 1–3% of global optimum. The marginal 1% improvement from SA/GA is outweighed by the computational cost, latency, and battery drain on embedded flight hardware.

**Metaheuristics become necessary when:**
- **Variable speed constraints** (VS-ATSP): UAV dynamically alters cruise speed for battery/wind balance.
- **Strict time windows** (ATSP-TW): Lines must be captured at specific times (solar angle, tidal level).
- **Extremely irregular boundaries** with deep concavities and many exclusion zones.

---

## 8. Multi-Block Routing (Hierarchical Decomposition)

### The Problem
Multi-polygon boundaries create disjoint flight blocks. A "flat" TSP treats all lines homogeneously → catastrophic block-switching (survey half of Block A, ferry 20 km to Block B for 3 lines, ferry back). Optimizes local entry angles at the expense of massive ferry fuel burn.

### Two-Level Hierarchical TSP

**Outer problem (block sequence):** Abstract each polygon as a single node. Solve a Generalized TSP (GTSP) or apply a metaheuristic to determine the optimal sequence of block visits. Cost matrix = wind-adjusted transit time between polygon centroids or closest bounding-box edges.

**Inner problem (line sequence per block):** Once block order is locked, solve an isolated ATSP for each block's flight lines using NN + Or-opt.

**Boundary condition:** The entry line of Block B must be optimized relative to the exit line of Block A. Dynamic programming treats entry/exit lines as variable terminals — the inner ATSP can rotate and reverse its line sequence to best align with the incoming ferry vector.

### Result
The aircraft exhausts all data acquisition in one geographic region before committing to the ferry transit to the next. High-cost ferry flights are isolated, and total mission profitability is maximized.

---

## 9. IronTrack Implementation Summary

| Scenario | Algorithm | Complexity | Notes |
|---|---|---|---|
| Parallel grid (simple polygon) | Boustrophedon sort | O(n log n) | Proven global optimum. Short-circuit the TSP. |
| Complex boundary (concave, exclusions) | NN + Or-opt | O(n²) | Sub-millisecond. 1–3% of optimal. |
| Multi-polygon disjoint blocks | 2-level hierarchical: GTSP outer + ATSP inner | O(k! × n²) | k = block count (typically ≤ 10) |
| Time windows / variable speed | SA or GA hybrid | O(varies) | Only when temporal constraints fracture the search space |

All routing computations are synchronous rayon work. If called from Axum endpoints, wrap in `tokio::task::spawn_blocking`.
