# ERC / DRC checklist — before any board goes to fab

Bob's list. Run it on every revision; paste the results into the design review.

## Schematic (ERC)

- [ ] Every power net driven: PWR_FLAG on VBUS and GND where the connector brings them in
- [ ] No unconnected pins without a no-connect flag
- [ ] Every symbol annotated (no `R?`), values filled in, footprints assigned
- [ ] Local labels spelt the same on both ends (`SDA` vs `sda` is two nets)
- [ ] Decoupling caps next to the supply pin they serve in the schematic too
- [ ] `Inspect → Electrical Rules Checker` reports 0 errors, 0 warnings

## Board (DRC)

- [ ] Rules: clearance 0.2 mm, track 0.25 mm, via 0.6/0.3 mm (fab's capability sheet)
- [ ] Ground zone filled, no islands, thermal reliefs on through-hole pads
- [ ] Keep-out under the USB-C shield tabs and the LDO tab
- [ ] Courtyards do not overlap; silkscreen off pads
- [ ] `Inspect → Design Rules Checker` with "refill zones" ticked: 0 errors, 0 unconnected
- [ ] Gerbers + drill regenerated after the last change (check the timestamp)

## Hand-off

- [ ] BOM CSV exported next to the project
- [ ] Netlist exported for the test fixture
- [ ] Rev bumped in the title block
