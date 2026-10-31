# House abbreviations (US structural practice)

The engine's table is `engine/kerf-core/src/house.rs` (`ABBREVIATIONS`); this page lists the same entries for review.

Source: how the 32 US Southwest drawing sets in the MASS reference library write these words (297 pages with text,
counted per written form). Rules:

- Shortened words take a period: `TYP.`, `CONT.`, `REINF.`, `SCHED.`. Contractions keep the apostrophe: `REQ'D`, `ADD'L`.
- Phrases use dotted initials: `O.C.`, `U.N.O.`, `E.N.`, `F.N.`, `B.N.`, `T.O.F.`, `A.B.`, `N.T.S.`.
- Material and product initialisms take no periods: `CMU`, `LVL`, `PSL`, `LSL`, `HSS`, `OSB`, `GLB`, `ICF`, `WWR`, `PT`.
- Symbols: `W/` with, `W/O` without, `@` at, `&` and, `#` bar size, `(2)` quantity.
- Always abbreviated (`W_NOTE_STYLE` asks for these): `O.C.`, `MIN.`, `MAX.`, `DIA.`, `GYP. BD.`. Each outnumbers the
  spelled-out word about 4:1 or more in the reference sets.
- Every other word may be spelled out or abbreviated. The sets do both (CONCRETE 228 : CONC. 61, BOTTOM 103 : BOT. 11,
  EACH 132 : EA. 122). Keep one choice within a detail.
- `dot` marks abbreviations that are also ordinary words (`SIM`, `COL`, `FIN`). They count as the abbreviation only
  when written with the period.

| Abbreviation | Meaning | Notes |
|---|---|---|
| `A.B.` | ANCHOR BOLT | 25× in refs |
| `ABV.` | ABOVE | 27× in refs |
| `ADD'L` | ADDITIONAL |  |
| `ADDL.` | ADDITIONAL |  |
| `ADJ.` | ADJACENT | 3× in refs |
| `A.F.F.` | ABOVE FINISH FLOOR | 1× in refs |
| `ALT.` | ALTERNATE / ALTERNATIVE | dot, 14× in refs |
| `APPROX.` | APPROXIMATE / APPROXIMATELY | 6× in refs |
| `ARCH.` | ARCHITECT / ARCHITECTURAL | dot, 44× in refs |
| `BD.` | BOARD | always, 16× in refs |
| `BLDG.` | BUILDING | 9× in refs |
| `BLK.` | BLOCK | 9× in refs |
| `BLK'G` | BLOCKING | 28× in refs |
| `BLKG.` | BLOCKING | 34× in refs |
| `BM.` | BEAM | 21× in refs |
| `B.N.` | BOUNDARY NAILING / BOUNDARY NAIL | 28× in refs |
| `B.O.` | BOTTOM OF | 3× in refs |
| `B.O.F.` | BOTTOM OF FOOTING |  |
| `BOT.` | BOTTOM | 11× in refs |
| `BOTT.` | BOTTOM |  |
| `BRG.` | BEARING | 1× in refs |
| `BTM.` | BOTTOM | 17× in refs |
| `BTWN.` | BETWEEN | 2× in refs |
| `C.I.P.` | CAST IN PLACE | 1× in refs |
| `C.J.` | CONTROL JOINT | 7× in refs |
| `CLG.` | CEILING | 29× in refs |
| `CLR.` | CLEAR | 60× in refs |
| `CMU` | CONCRETE MASONRY UNIT | 98× in refs |
| `COL.` | COLUMN | dot, 26× in refs |
| `CONC.` | CONCRETE | 61× in refs |
| `CONN.` | CONNECTION | 26× in refs |
| `CONST.` | CONSTRUCTION |  |
| `CONT.` | CONTINUOUS | 114× in refs |
| `CTR.` | CENTER | 1× in refs |
| `DBL.` | DOUBLE | 39× in refs |
| `DET.` | DETAIL | 22× in refs |
| `D.F.` | DOUGLAS FIR | 13× in refs |
| `DF-L` | DOUGLAS FIR-LARCH |  |
| `DIA.` | DIAMETER | always, 117× in refs |
| `DIAG.` | DIAGONAL | 6× in refs |
| `DIM.` | DIMENSION | dot, 9× in refs |
| `DN.` | DOWN | 39× in refs |
| `DWG.` | DRAWING | 2× in refs |
| `DWGS.` | DRAWINGS | 20× in refs |
| `EA.` | EACH | 122× in refs |
| `E.F.` | EACH FACE |  |
| `E.J.` | EXPANSION JOINT | 1× in refs |
| `EL.` | ELEVATION | 2× in refs |
| `ELEC.` | ELECTRICAL | 4× in refs |
| `ELEV.` | ELEVATION | 1× in refs |
| `EMBED.` | EMBEDMENT | 31× in refs |
| `E.N.` | EDGE NAILING / EDGE NAIL | 56× in refs |
| `ENGR.` | ENGINEER | 1× in refs |
| `E.O.R.` | ENGINEER OF RECORD | 4× in refs |
| `EOR` | ENGINEER OF RECORD |  |
| `EQ.` | EQUAL | 49× in refs |
| `E.W.` | EACH WAY | 11× in refs |
| `EXIST.` | EXISTING | 1× in refs |
| `EXIST'G` | EXISTING |  |
| `EXP.` | EXPANSION | dot, 7× in refs |
| `EXT.` | EXTERIOR | 35× in refs |
| `FDN.` | FOUNDATION | 12× in refs |
| `F.F.` | FINISH FLOOR | 38× in refs |
| `FIN.` | FINISH / FINISHED | dot, 9× in refs |
| `FLR.` | FLOOR | 4× in refs |
| `F.N.` | FIELD NAILING / FIELD NAIL | 7× in refs |
| `FTG.` | FOOTING | 17× in refs |
| `GA.` | GAUGE / GAGE | 37× in refs |
| `GALV.` | GALVANIZED | 9× in refs |
| `GLB` | GLUED LAMINATED BEAM / GLULAM BEAM | 16× in refs |
| `GR.` | GRADE | 1× in refs |
| `GRD.` | GRADE |  |
| `GYP.` | GYPSUM | always, 87× in refs |
| `HD` | HOLDOWN / HOLD-DOWN | 12× in refs |
| `HDR.` | HEADER | 15× in refs |
| `HGR.` | HANGER |  |
| `HORIZ.` | HORIZONTAL | 14× in refs |
| `HSS` | HOLLOW STRUCTURAL SECTION | 19× in refs |
| `HT.` | HEIGHT | 10× in refs |
| `ICF` | INSULATED CONCRETE FORM | 1× in refs |
| `I.D.` | INSIDE DIAMETER | 1× in refs |
| `INSUL.` | INSULATION | 6× in refs |
| `INT.` | INTERIOR | dot, 17× in refs |
| `JST.` | JOIST | 1× in refs |
| `JT.` | JOINT | 1× in refs |
| `LG.` | LONG |  |
| `L.L.H.` | LONG LEG HORIZONTAL | 1× in refs |
| `L.L.V.` | LONG LEG VERTICAL | 1× in refs |
| `LSL` | LAMINATED STRAND LUMBER | 4× in refs |
| `LVL` | LAMINATED VENEER LUMBER | 36× in refs |
| `MAS.` | MASONRY | 4× in refs |
| `MAX.` | MAXIMUM | always, 107× in refs |
| `M.B.` | MACHINE BOLT |  |
| `MECH.` | MECHANICAL | 16× in refs |
| `MFR.` | MANUFACTURER | 29× in refs |
| `MFR'S` | MANUFACTURER |  |
| `MIN.` | MINIMUM | always, 456× in refs |
| `MISC.` | MISCELLANEOUS | 5× in refs |
| `MTL.` | METAL |  |
| `NOM.` | NOMINAL |  |
| `N.S.` | NON-SHRINK | 5× in refs |
| `N.T.S.` | NOT TO SCALE | 25× in refs |
| `NTS` | NOT TO SCALE | 43× in refs |
| `O.C.` | ON CENTER | always, 537× in refs |
| `O.D.` | OUTSIDE DIAMETER | 1× in refs |
| `OPNG.` | OPENING | 1× in refs |
| `OPP.` | OPPOSITE | 4× in refs |
| `OSB` | ORIENTED STRAND BOARD | 28× in refs |
| `P.A.F.` | POWER-ACTUATED FASTENER | 1× in refs |
| `PERP.` | PERPENDICULAR | 10× in refs |
| `PL` | PLATE | 13× in refs |
| `PL.` | PLATE | 57× in refs |
| `PLY.` | PLYWOOD | 4× in refs |
| `PLYWD.` | PLYWOOD | 2× in refs |
| `PSL` | PARALLEL STRAND LUMBER | 8× in refs |
| `P.T.` | PRESSURE TREATED | 17× in refs |
| `PT` | PRESSURE TREATED | 2× in refs |
| `REF.` | REFERENCE | 14× in refs |
| `REINF.` | REINFORCING / REINFORCEMENT / REINFORCED | 38× in refs |
| `REQ'D` | REQUIRED | 23× in refs |
| `REQD.` | REQUIRED | 1× in refs |
| `S.A.D.` | SEE ARCHITECTURAL DRAWINGS |  |
| `SCHED.` | SCHEDULE | 37× in refs |
| `SECT.` | SECTION | 1× in refs |
| `SHT.` | SHEET | 1× in refs |
| `SHTG.` | SHEATHING | 56× in refs |
| `SIM.` | SIMILAR | dot, 23× in refs |
| `S.O.G.` | SLAB ON GRADE |  |
| `SPA.` | SPACES / SPACING / SPACED | dot, 4× in refs |
| `SPEC.` | SPECIFICATION | 4× in refs |
| `SPECS.` | SPECIFICATIONS | 19× in refs |
| `SQ.` | SQUARE | 54× in refs |
| `S.S.` | STAINLESS STEEL | 1× in refs |
| `STAG.` | STAGGERED / STAGGER | dot, 2× in refs |
| `STD.` | STANDARD | 12× in refs |
| `STIFF.` | STIFFENER | dot |
| `STL.` | STEEL | 3× in refs |
| `STRUC.` | STRUCTURAL | 1× in refs |
| `STRUCT.` | STRUCTURAL | 8× in refs |
| `T&B` | TOP AND BOTTOM | 5× in refs |
| `T&G` | TONGUE AND GROOVE | 7× in refs |
| `THK.` | THICK / THICKNESS | 30× in refs |
| `THRU` | THROUGH | 21× in refs |
| `T.O.` | TOP OF | 21× in refs |
| `T.O.C.` | TOP OF CONCRETE | 1× in refs |
| `T.O.F.` | TOP OF FOOTING |  |
| `T.O.M.` | TOP OF MASONRY | 1× in refs |
| `T.O.S.` | TOP OF SLAB | 2× in refs |
| `T.O.W.` | TOP OF WALL | 3× in refs |
| `TYP.` | TYPICAL | 234× in refs |
| `U.N.O.` | UNLESS NOTED OTHERWISE | 92× in refs |
| `UNO` | UNLESS NOTED OTHERWISE | 68× in refs |
| `VERT.` | VERTICAL | 26× in refs |
| `V.I.F.` | VERIFY IN FIELD | 1× in refs |
| `WD.` | WOOD | 4× in refs |
| `WWR` | WELDED WIRE REINFORCEMENT |  |
