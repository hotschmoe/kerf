# House abbreviations (US structural practice)

The engine's table is `engine/kerf-core/src/house.rs` (`ABBREVIATIONS`: the approved forms, `VARIANTS`: the other
spellings in use and the approved form each maps to). This page is generated from it: `kerf schema abbreviations`
prints the same table for the company style in force (`kerf.kerfstyle.json`, SPEC 25.6). After changing the table,
replace everything from the `List:` line down with the output of
`kerf schema abbreviations --style spec/styles/kerf-standard.kerfstyle.json` (from its `List:` line).

Source: how the 32 US Southwest drawing sets in the MASS reference library write these words (297 pages with text,
counted per written form). Policy (MASS principal engineer, SPEC 25.3, 25.6):

- Spelling a word out or abbreviating it is the author's choice (CONCRETE 228 : CONC. 61, BOTTOM 103 : BOT. 11,
  EACH 132 : EA. 122 in the sets). Nothing forces an abbreviation unless the company style lists it in `always`.
- An abbreviation is written in its approved form, else W_ABBREV names the rule and the fix:
  shortened words take a period (`TYP.`, `CONT.`, `MIN.`); phrases take dotted initials (`O.C.`, `U.N.O.`, `E.O.R.`,
  `N.T.S.`, `E.N.`, `T.O.F.`); material and product acronyms take none (`CMU`, `LVL`, `OSB`, `HSS`, `PL`);
  contractions keep the apostrophe (`REQ'D`, `ADD'L`); a variant takes the form it maps to (`BOTT.`/`BTM.` -> `BOT.`,
  `PT` -> `P.T.` since PT also reads as post-tensioned).
- Where two spellings were in use the table keeps the one the sets use most, except the engineer's choices `BOT.`, `P.T.`,
  `N.T.S.`, `U.N.O.`, `E.O.R.` (dotted initials for phrases), `PL`, `BLKG.`, `ADD'L`, `REQ'D`, `EXIST.`, `PLYWD.`, `MFR.`.
- A bare form that is also an ordinary word (`SIM`, `COL`, `FIN`, `TO`, `ID`, `EMBED`, `PLY`) counts as the
  abbreviation only with its periods and is never flagged on its own.
- Symbols: `W/` with, `W/O` without, `@` at, `&` and, `#` bar size, `(2)` quantity.

List: us-structural (built in). Rules (notes.abbreviations.rules): period_on_shortened on, dotted_initials on, bare_acronyms on, variants on. Always abbreviated (notes.abbreviations.always): none.

Refs: how often the 32 reference sets write that spelling (blank: not counted).

| Approved | Refs | Meaning | Form | Other spellings W_ABBREV flags (refs, rule) |
|---|--:|---|---|---|
| `A.B.` | 25 | ANCHOR BOLT | dotted initials | `AB` (dotted_initials) |
| `ABV.` | 27 | ABOVE | shortened word | `ABV` (period_on_shortened) |
| `ADD'L` |  | ADDITIONAL | contraction | `ADDL.` (variants) |
| `ADJ.` | 3 | ADJACENT | shortened word | `ADJ` (period_on_shortened) |
| `A.F.F.` | 1 | ABOVE FINISH FLOOR | dotted initials | `AFF` (dotted_initials) |
| `ALT.` | 14 | ALTERNATE / ALTERNATIVE | shortened word | `ALT` is an ordinary word: never flagged |
| `APPROX.` | 6 | APPROXIMATE / APPROXIMATELY | shortened word | `APPROX` (period_on_shortened) |
| `ARCH.` | 44 | ARCHITECT / ARCHITECTURAL | shortened word | `ARCH` is an ordinary word: never flagged |
| `BD.` | 16 | BOARD | shortened word | `BD` (period_on_shortened) |
| `BLDG.` | 9 | BUILDING | shortened word | `BLDG` (period_on_shortened) |
| `BLK.` | 9 | BLOCK | shortened word | `BLK` (period_on_shortened) |
| `BLKG.` | 34 | BLOCKING | shortened word | `BLK'G` (28, variants), `BLKG` (period_on_shortened) |
| `BM.` | 21 | BEAM | shortened word | `BM` (period_on_shortened) |
| `B.N.` | 28 | BOUNDARY NAILING / BOUNDARY NAIL | dotted initials | `BN` (dotted_initials) |
| `B.O.` | 3 | BOTTOM OF | dotted initials | `BO` (dotted_initials) |
| `B.O.F.` |  | BOTTOM OF FOOTING | dotted initials | `BOF` (dotted_initials) |
| `BOT.` | 11 | BOTTOM | shortened word | `BOTT.` (variants), `BTM.` (17, variants), `BOT` (period_on_shortened) |
| `BRG.` | 1 | BEARING | shortened word | `BRG` (period_on_shortened) |
| `BTWN.` | 2 | BETWEEN | shortened word | `BTWN` (period_on_shortened) |
| `CALC.` |  | CALCULATION / CALCULATIONS | shortened word | `CALC` (period_on_shortened) |
| `C.I.P.` | 1 | CAST IN PLACE | dotted initials | `CIP` (dotted_initials) |
| `C.J.` | 7 | CONTROL JOINT | dotted initials | `CJ` (dotted_initials) |
| `CLG.` | 29 | CEILING | shortened word | `CLG` (period_on_shortened) |
| `CLR.` | 60 | CLEAR | shortened word | `CLR` (period_on_shortened) |
| `CMU` | 98 | CONCRETE MASONRY UNIT | acronym | `C.M.U.` (bare_acronyms) |
| `COL.` | 26 | COLUMN | shortened word | `COL` is an ordinary word: never flagged |
| `CONC.` | 61 | CONCRETE | shortened word | `CONC` (period_on_shortened) |
| `CONN.` | 26 | CONNECTION | shortened word | `CONN` (period_on_shortened) |
| `CONST.` |  | CONSTRUCTION | shortened word | `CONST` (period_on_shortened) |
| `CONT.` | 114 | CONTINUOUS | shortened word | `CONT` (period_on_shortened) |
| `CTR.` | 1 | CENTER | shortened word | `CTR` (period_on_shortened) |
| `DBL.` | 39 | DOUBLE | shortened word | `DBL` (period_on_shortened) |
| `DET.` | 22 | DETAIL | shortened word | `DET` (period_on_shortened) |
| `D.F.` | 13 | DOUGLAS FIR | dotted initials | `DF` (dotted_initials) |
| `DF-L` |  | DOUGLAS FIR-LARCH | symbol |  |
| `DIA.` | 117 | DIAMETER | shortened word | `DIA` (period_on_shortened) |
| `DIAG.` | 6 | DIAGONAL | shortened word | `DIAG` (period_on_shortened) |
| `DIM.` | 9 | DIMENSION | shortened word | `DIM` is an ordinary word: never flagged |
| `DN.` | 39 | DOWN | shortened word | `DN` (period_on_shortened) |
| `DWG.` | 2 | DRAWING | shortened word | `DWG` (period_on_shortened) |
| `DWGS.` | 20 | DRAWINGS | shortened word | `DWGS` (period_on_shortened) |
| `EA.` | 122 | EACH | shortened word | `EA` (period_on_shortened) |
| `E.F.` |  | EACH FACE | dotted initials | `EF` (dotted_initials) |
| `E.J.` | 1 | EXPANSION JOINT | dotted initials | `EJ` (dotted_initials) |
| `EL.` | 2 | ELEVATION | shortened word | `ELEV.` (1, variants), `EL` (period_on_shortened) |
| `ELEC.` | 4 | ELECTRICAL | shortened word | `ELEC` (period_on_shortened) |
| `EMBED.` | 31 | EMBEDMENT | shortened word | `EMBED` is an ordinary word: never flagged |
| `E.N.` | 56 | EDGE NAILING / EDGE NAIL | dotted initials | `EN` (dotted_initials) |
| `ENGR.` | 1 | ENGINEER | shortened word | `ENGR` (period_on_shortened) |
| `E.O.R.` | 4 | ENGINEER OF RECORD | dotted initials | `EOR` (dotted_initials) |
| `EQ.` | 49 | EQUAL | shortened word | `EQ` (period_on_shortened) |
| `E.W.` | 11 | EACH WAY | dotted initials | `EW` (dotted_initials) |
| `EXIST.` | 1 | EXISTING | shortened word | `EXIST'G` (variants), `EXIST` is an ordinary word: never flagged |
| `EXP.` | 7 | EXPANSION | shortened word | `EXP` is an ordinary word: never flagged |
| `EXT.` | 35 | EXTERIOR | shortened word | `EXT` (period_on_shortened) |
| `FDN.` | 12 | FOUNDATION | shortened word | `FDN` (period_on_shortened) |
| `F.F.` | 38 | FINISH FLOOR | dotted initials | `FF` (dotted_initials) |
| `FIN.` | 9 | FINISH / FINISHED | shortened word | `FIN` is an ordinary word: never flagged |
| `FLR.` | 4 | FLOOR | shortened word | `FLR` (period_on_shortened) |
| `F.N.` | 7 | FIELD NAILING / FIELD NAIL | dotted initials | `FN` (dotted_initials) |
| `FTG.` | 17 | FOOTING | shortened word | `FTG` (period_on_shortened) |
| `GA.` | 37 | GAUGE / GAGE | shortened word | `GA` (period_on_shortened) |
| `GALV.` | 9 | GALVANIZED | shortened word | `GALV` (period_on_shortened) |
| `GLB` | 16 | GLUED LAMINATED BEAM / GLULAM BEAM | acronym | `G.L.B.` (bare_acronyms) |
| `GR.` | 1 | GRADE | shortened word | `GRD.` (variants), `GR` (period_on_shortened) |
| `GYP.` | 87 | GYPSUM | shortened word | `GYP` (period_on_shortened) |
| `HD` | 12 | HOLDOWN / HOLD-DOWN | acronym | `H.D.` (bare_acronyms) |
| `HDR.` | 15 | HEADER | shortened word | `HDR` (period_on_shortened) |
| `HGR.` |  | HANGER | shortened word | `HGR` (period_on_shortened) |
| `HORIZ.` | 14 | HORIZONTAL | shortened word | `HORIZ` (period_on_shortened) |
| `HSS` | 19 | HOLLOW STRUCTURAL SECTION | acronym | `H.S.S.` (bare_acronyms) |
| `HT.` | 10 | HEIGHT | shortened word | `HT` (period_on_shortened) |
| `ICF` | 1 | INSULATED CONCRETE FORM | acronym | `I.C.F.` (bare_acronyms) |
| `I.D.` | 1 | INSIDE DIAMETER | dotted initials | `ID` is an ordinary word: never flagged |
| `INSUL.` | 6 | INSULATION | shortened word | `INSUL` (period_on_shortened) |
| `INT.` | 17 | INTERIOR | shortened word | `INT` is an ordinary word: never flagged |
| `JST.` | 1 | JOIST | shortened word | `JST` (period_on_shortened) |
| `JT.` | 1 | JOINT | shortened word | `JT` (period_on_shortened) |
| `LG.` |  | LONG | shortened word | `LG` (period_on_shortened) |
| `L.L.H.` | 1 | LONG LEG HORIZONTAL | dotted initials | `LLH` (dotted_initials) |
| `L.L.V.` | 1 | LONG LEG VERTICAL | dotted initials | `LLV` (dotted_initials) |
| `LSL` | 4 | LAMINATED STRAND LUMBER | acronym | `L.S.L.` (bare_acronyms) |
| `LVL` | 36 | LAMINATED VENEER LUMBER | acronym | `L.V.L.` (bare_acronyms) |
| `MAS.` | 4 | MASONRY | shortened word | `MAS` (period_on_shortened) |
| `MAX.` | 107 | MAXIMUM | shortened word | `MAX` (period_on_shortened) |
| `M.B.` |  | MACHINE BOLT | dotted initials | `MB` (dotted_initials) |
| `MECH.` | 16 | MECHANICAL | shortened word | `MECH` (period_on_shortened) |
| `MFR.` | 29 | MANUFACTURER | shortened word | `MFR'S` (variants), `MFR` (period_on_shortened) |
| `MIN.` | 456 | MINIMUM | shortened word | `MIN` (period_on_shortened) |
| `MISC.` | 5 | MISCELLANEOUS | shortened word | `MISC` (period_on_shortened) |
| `MTL.` |  | METAL | shortened word | `MTL` (period_on_shortened) |
| `NOM.` |  | NOMINAL | shortened word | `NOM` (period_on_shortened) |
| `N.S.` | 5 | NON-SHRINK | dotted initials | `NS` (dotted_initials) |
| `N.T.S.` | 25 | NOT TO SCALE | dotted initials | `NTS` (43, dotted_initials) |
| `O.C.` | 537 | ON CENTER | dotted initials | `OC` (dotted_initials) |
| `O.D.` | 1 | OUTSIDE DIAMETER | dotted initials | `OD` (dotted_initials) |
| `OPNG.` | 1 | OPENING | shortened word | `OPNG` (period_on_shortened) |
| `OPP.` | 4 | OPPOSITE | shortened word | `OPP` (period_on_shortened) |
| `OSB` | 28 | ORIENTED STRAND BOARD | acronym | `O.S.B.` (bare_acronyms) |
| `P.A.F.` | 1 | POWER-ACTUATED FASTENER | dotted initials | `PAF` (dotted_initials) |
| `PERP.` | 10 | PERPENDICULAR | shortened word | `PERP` (period_on_shortened) |
| `PL` | 13 | PLATE | acronym | `PL.` (57, bare_acronyms) |
| `PLYWD.` | 2 | PLYWOOD | shortened word | `PLY.` (4, variants), `PLYWD` (period_on_shortened) |
| `PSL` | 8 | PARALLEL STRAND LUMBER | acronym | `P.S.L.` (bare_acronyms) |
| `P.T.` | 17 | PRESSURE TREATED | dotted initials | `PT` (2, dotted_initials) |
| `REF.` | 14 | REFERENCE | shortened word | `REF` (period_on_shortened) |
| `REINF.` | 38 | REINFORCING / REINFORCEMENT / REINFORCED | shortened word | `REINF` (period_on_shortened) |
| `REQ'D` | 23 | REQUIRED | contraction | `REQD` (variants), `REQD.` (1, variants) |
| `S.A.D.` |  | SEE ARCHITECTURAL DRAWINGS | dotted initials | `SAD` is an ordinary word: never flagged |
| `SCHED.` | 37 | SCHEDULE | shortened word | `SCHED` (period_on_shortened) |
| `SECT.` | 1 | SECTION | shortened word | `SECT` is an ordinary word: never flagged |
| `SHT.` | 1 | SHEET | shortened word | `SHT` (period_on_shortened) |
| `SHTG.` | 56 | SHEATHING | shortened word | `SHTG` (period_on_shortened) |
| `SIM.` | 23 | SIMILAR | shortened word | `SIM` is an ordinary word: never flagged |
| `S.O.G.` |  | SLAB ON GRADE | dotted initials | `SOG` (dotted_initials) |
| `SPA.` | 4 | SPACES / SPACING / SPACED | shortened word | `SPA` is an ordinary word: never flagged |
| `SPEC.` | 4 | SPECIFICATION | shortened word | `SPEC` (period_on_shortened) |
| `SPECS.` | 19 | SPECIFICATIONS | shortened word | `SPECS` (period_on_shortened) |
| `SQ.` | 54 | SQUARE | shortened word | `SQ` (period_on_shortened) |
| `S.S.` | 1 | STAINLESS STEEL | dotted initials | `SS` (dotted_initials) |
| `STAG.` | 2 | STAGGERED / STAGGER | shortened word | `STAG` is an ordinary word: never flagged |
| `STD.` | 12 | STANDARD | shortened word | `STD` (period_on_shortened) |
| `STIFF.` |  | STIFFENER | shortened word | `STIFF` is an ordinary word: never flagged |
| `STL.` | 3 | STEEL | shortened word | `STL` (period_on_shortened) |
| `STRUCT.` | 8 | STRUCTURAL | shortened word | `STRUC.` (1, variants), `STRUCT` (period_on_shortened) |
| `T&B` | 5 | TOP AND BOTTOM | symbol |  |
| `T&G` | 7 | TONGUE AND GROOVE | symbol |  |
| `THK.` | 30 | THICK / THICKNESS | shortened word | `THK` (period_on_shortened) |
| `THRU` | 21 | THROUGH | acronym | `T.H.R.U.` (bare_acronyms) |
| `T.O.` | 21 | TOP OF | dotted initials | `TO` is an ordinary word: never flagged |
| `T.O.C.` | 1 | TOP OF CONCRETE | dotted initials | `TOC` (dotted_initials) |
| `T.O.F.` |  | TOP OF FOOTING | dotted initials | `TOF` (dotted_initials) |
| `T.O.M.` | 1 | TOP OF MASONRY | dotted initials | `TOM` is an ordinary word: never flagged |
| `T.O.S.` | 2 | TOP OF SLAB | dotted initials | `TOS` (dotted_initials) |
| `T.O.W.` | 3 | TOP OF WALL | dotted initials | `TOW` is an ordinary word: never flagged |
| `TYP.` | 234 | TYPICAL | shortened word | `TYP` (period_on_shortened) |
| `U.N.O.` | 92 | UNLESS NOTED OTHERWISE | dotted initials | `UNO` (68, dotted_initials) |
| `VERT.` | 26 | VERTICAL | shortened word | `VERT` (period_on_shortened) |
| `V.I.F.` | 1 | VERIFY IN FIELD | dotted initials | `VIF` (dotted_initials) |
| `WD.` | 4 | WOOD | shortened word | `WD` (period_on_shortened) |
| `WWR` |  | WELDED WIRE REINFORCEMENT | acronym | `W.W.R.` (bare_acronyms) |
