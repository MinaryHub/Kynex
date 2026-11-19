# KYNEX screenshots

1920 × 1080, KYNEX 0.4.1, the viewer in English, for store and app listings.

| File | Shows |
|---|---|
| `01-factory-simulation.png` | A beverage bottling plant running: each station's state (RUN / IDLE / DOWN), utilization and output over it, the bottleneck in red, splits and merges in the flow, the finished-goods warehouse at the far end |
| `02-timeline-utilization.png` | The timeline: what each machine and AMR did over the run, and its utilization, with the plant running above it |
| `03-statistics.png` | Statistics: output, WIP, lead time, the bottleneck, state by equipment and cumulative parts made |
| `04-link-mode.png` | Link mode (Shift+C): a line of process tables, linked in turn; the last one is linked by pressing on a station and dragging onto the next |
| `05-split-merge.png` | A split shared 70 : 30 between two ways that merge again, set from the split's sign |
| `06-amr-transport-lanes.png` | An AMR carrying parts between stations, numbered AMR stops (P1–P3) and a lane's menu: both ways or one way |
| `07-robot-cell.png` | A robot on its pedestal carrying the flow's parts from a cart to a station, with its joints and TCP in the robot panel |
| `08-car-body-plant.png` | A car plant: the press tandem (the bottleneck), panel racks, welding cells, the body buffer, E-coat and paint booths |
| `09-machining-molding-lines.png` | Two lines fed by CNC machining centres and an injection molding machine, robots beside them, operators at the stations, buffers, a split and merges |
| `10-factory-interior.png` | A factory hall: an overhead crane, pallet racks, a forklift, CNC machines, the press shop and AMR lanes across the floor |

Taken from the demo plants and samples in `apps/kynex/demos/`. `04` and `05` are
the sample `01-line-on-plan` with one link left out (`04`) or Station 1 split
70 : 30 between Buffer 1 and Station 2 (`05`).
