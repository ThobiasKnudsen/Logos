//! Build-time patcher for Geist Mono's broken multi-character ligatures.
//!
//! Geist Mono ships its N-character ligatures (`:=`, `!=`, `<=`, `->`, etc.)
//! with `advance = 600` (one character wide) and `lsb ≈ -400` (visual extends
//! leftward, into the preceding character's cell). HarfBuzz follows the
//! standard convention of placing a ligature glyph at the FIRST consumed
//! glyph's pen position, so the leftward overhang collides with whatever
//! precedes it — typing `e:=` shows `:=` overlapping the `e`.
//!
//! Fix: for every `.liga` glyph whose advance is less than `N * 600`, shift
//! the outline rightward so it sits centered in an N-character cell, and set
//! advance to `N * 600`. Combining marks (advance == 0) are left alone.

use std::collections::HashMap;

use read_fonts::tables::glyf::CurvePoint;
use read_fonts::tables::gsub::SubstitutionSubtables;
use read_fonts::types::GlyphId;
use read_fonts::{FontRef, TableProvider};
use write_fonts::from_obj::FromTableRef;
use write_fonts::tables::glyf::{
    Anchor, Bbox, Component, CompositeGlyph, Contour, GlyfLocaBuilder, Glyph,
};
use write_fonts::tables::head::Head;
use write_fonts::tables::hmtx::{Hmtx, LongMetric};
use write_fonts::tables::loca::LocaFormat;
use write_fonts::FontBuilder;

/// Monospace base advance for Geist Mono. Every plain glyph in the font
/// advances by this much; an N-character ligature should advance by N times
/// this.
const BASE_ADVANCE: u16 = 600;

pub fn patch_geist_mono(bytes: &[u8]) -> Vec<u8> {
    let font = FontRef::new(bytes).expect("parse font");

    // 1. Walk GSUB and collect every ligature glyph with its component count.
    //    We track the MAX component count seen, since a glyph could (in
    //    theory) appear as the substitution target of multiple ligatures.
    let mut lig_components: HashMap<u16, usize> = HashMap::new();
    if let Ok(gsub) = font.gsub() {
        let lookup_list = gsub.lookup_list().expect("gsub lookup list");
        for lookup in lookup_list.lookups().iter() {
            let lookup = lookup.expect("lookup");
            let subtables = match lookup.subtables() {
                Ok(s) => s,
                Err(_) => continue,
            };
            if let SubstitutionSubtables::Ligature(subs) = subtables {
                for sub in subs.iter() {
                    let sub = sub.expect("ligature subtable");
                    for ligset in sub.ligature_sets().iter() {
                        let ligset = ligset.expect("ligature set");
                        for lig in ligset.ligatures().iter() {
                            let lig = lig.expect("ligature");
                            let gid = lig.ligature_glyph().to_u16();
                            let n = 1 + lig.component_glyph_ids().len();
                            let prev = lig_components.get(&gid).copied().unwrap_or(0);
                            lig_components.insert(gid, prev.max(n));
                        }
                    }
                }
            }
        }
    }

    // 2. Decide which ligatures need patching by comparing current advance to
    //    the expected N*BASE_ADVANCE. Skip zero-advance glyphs (combining
    //    marks).
    let hmtx_in = font.hmtx().expect("hmtx");
    let h_metrics = hmtx_in.h_metrics();
    let lsbs = hmtx_in.left_side_bearings();
    let num_long = h_metrics.len();
    let last_advance = h_metrics
        .last()
        .map(|m| m.advance())
        .unwrap_or(BASE_ADVANCE);
    let num_glyphs = font.maxp().expect("maxp").num_glyphs() as usize;

    let metric = |gid: usize| -> (u16, i16) {
        if gid < num_long {
            (h_metrics[gid].advance(), h_metrics[gid].side_bearing())
        } else {
            (last_advance, lsbs[gid - num_long].get())
        }
    };

    // glyph id -> expected advance (only set for glyphs we're patching).
    let mut new_advance: HashMap<u16, u16> = HashMap::new();
    for (&gid, &n) in &lig_components {
        let (aw, _lsb) = metric(gid as usize);
        if aw == 0 {
            continue;
        }
        let expected = (n as u16) * BASE_ADVANCE;
        if aw < expected {
            new_advance.insert(gid, expected);
        }
    }

    // 3. Walk every glyph. For patched ones, compute a shift that centers
    //    the visual inside the new N-cell box, apply it to the outline (or
    //    composite component anchors), and recompute the bounding box.
    let glyf_in = font.glyf().expect("glyf");
    let loca_in = font.loca(None).expect("loca");

    // Track per-glyph new lsb (= new x_min after shift) so hmtx matches.
    let mut new_lsb: HashMap<u16, i16> = HashMap::new();

    let mut builder = GlyfLocaBuilder::new();
    for gid_u16 in 0..num_glyphs as u16 {
        let gid = GlyphId::new(gid_u16 as u32);
        let read_glyph = loca_in.get_glyf(gid, &glyf_in).expect("glyf entry");
        let mut wglyph = match read_glyph {
            None => Glyph::Empty,
            Some(g) => Glyph::from_table_ref(&g),
        };

        if let Some(&expected_advance) = new_advance.get(&gid_u16) {
            let (cur_xmin, cur_xmax) = match &wglyph {
                Glyph::Simple(s) => (s.bbox.x_min as i32, s.bbox.x_max as i32),
                Glyph::Composite(c) => (c.bbox.x_min as i32, c.bbox.x_max as i32),
                Glyph::Empty => (0, 0),
            };
            let visual_w = cur_xmax - cur_xmin;
            let cell_w = expected_advance as i32;
            // Center the visual horizontally within the cell. If the visual
            // is wider than the cell (rare but possible for very wide
            // ligatures like `===`), leave a positive margin on each side
            // anyway — the result still won't collide leftward with the
            // preceding char.
            let target_xmin = ((cell_w - visual_w) / 2).max(0);
            let shift = (target_xmin - cur_xmin) as i16;

            match &mut wglyph {
                Glyph::Simple(s) => {
                    let new_contours: Vec<Contour> = s
                        .contours
                        .iter()
                        .map(|c| {
                            let pts: Vec<CurvePoint> = c
                                .iter()
                                .map(|p| CurvePoint {
                                    x: p.x.saturating_add(shift),
                                    y: p.y,
                                    on_curve: p.on_curve,
                                })
                                .collect();
                            Contour::from(pts)
                        })
                        .collect();
                    s.contours = new_contours;
                    s.recompute_bounding_box();
                    new_lsb.insert(gid_u16, s.bbox.x_min);
                }
                Glyph::Composite(c) => {
                    // Build a fresh CompositeGlyph with shifted Offset
                    // anchors, then overwrite its bbox with the shifted
                    // original (we already know what the bbox should be).
                    let comps: Vec<(Component, Bbox)> = c
                        .components()
                        .iter()
                        .map(|comp| {
                            let new_anchor = match comp.anchor {
                                Anchor::Offset { x, y } => Anchor::Offset {
                                    x: x.saturating_add(shift),
                                    y,
                                },
                                other => other,
                            };
                            (
                                Component {
                                    glyph: comp.glyph,
                                    anchor: new_anchor,
                                    flags: comp.flags,
                                    transform: comp.transform,
                                },
                                Bbox::default(),
                            )
                        })
                        .collect();
                    let new_bbox = Bbox {
                        x_min: c.bbox.x_min.saturating_add(shift),
                        y_min: c.bbox.y_min,
                        x_max: c.bbox.x_max.saturating_add(shift),
                        y_max: c.bbox.y_max,
                    };
                    let mut new_comp =
                        CompositeGlyph::try_from_iter(comps).expect("composite has components");
                    new_comp.bbox = new_bbox;
                    new_lsb.insert(gid_u16, new_bbox.x_min);
                    wglyph = Glyph::Composite(new_comp);
                }
                Glyph::Empty => {}
            }
        }

        builder.add_glyph(&wglyph).expect("add glyph");
    }

    let (new_glyf, new_loca, loca_format) = builder.build();

    // 4. Rebuild hmtx. Expand to one LongMetric per glyph so callers don't
    //    have to reason about the shared-advance compression — the size
    //    overhead is negligible.
    let mut metrics: Vec<LongMetric> = Vec::with_capacity(num_glyphs);
    for gid_u16 in 0..num_glyphs as u16 {
        let (aw, lsb) = metric(gid_u16 as usize);
        let advance = new_advance.get(&gid_u16).copied().unwrap_or(aw);
        let side_bearing = new_lsb.get(&gid_u16).copied().unwrap_or(lsb);
        metrics.push(LongMetric::new(advance, side_bearing));
    }
    let new_hmtx = Hmtx::new(metrics, Vec::new());

    // 5. Patch the `head` table: index_to_loc_format must match the new
    //    loca format chosen by the builder. We copy every other field from
    //    the source.
    let head_in = font.head().expect("head");
    let mut new_head = Head::from_table_ref(&head_in);
    new_head.index_to_loc_format = match loca_format {
        LocaFormat::Short => 0,
        LocaFormat::Long => 1,
    };

    // 6. Patch the `hhea` table: number_of_h_metrics must match the new
    //    hmtx (we expanded to one LongMetric per glyph).
    let hhea_in = font.hhea().expect("hhea");
    let mut new_hhea = write_fonts::tables::hhea::Hhea::from_table_ref(&hhea_in);
    new_hhea.number_of_h_metrics = num_glyphs as u16;

    // 7. Assemble the new font, copying every other table unchanged.
    let mut fb = FontBuilder::new();
    fb.add_table(&new_glyf).expect("glyf");
    fb.add_table(&new_loca).expect("loca");
    fb.add_table(&new_hmtx).expect("hmtx");
    fb.add_table(&new_head).expect("head");
    fb.add_table(&new_hhea).expect("hhea");
    fb.copy_missing_tables(font);
    fb.build()
}
