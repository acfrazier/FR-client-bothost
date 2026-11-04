//! Explicit Java-to-Rust field projections. Production code has no serde dependency.
use super::config_377::*;
use serde_json::{json, Value};

macro_rules! fields {
    ($v:ident; $($java:literal => $rust:ident),* $(,)?) => {
        json!({$($java: &$v.$rust),*})
    };
}

fn signed_bytes(bytes: &Option<Vec<u8>>) -> Value {
    json!(bytes.as_ref().map(|bytes| bytes
        .iter()
        .map(|&byte| i32::from(byte as i8))
        .collect::<Vec<_>>()))
}

pub fn seq(v: &SeqType377) -> Value {
    fields!(v; "field776"=>num_frames,"field777"=>frames,"field778"=>iframes,"field779"=>delay,
        "field780"=>loops,"field781"=>walkmerge,"field782"=>reachforward,"field783"=>priority,
        "field784"=>replaceheldleft,"field785"=>replaceheldright,"field786"=>maxloops,
        "field787"=>preanim_move,"field788"=>postanim_move,"field789"=>duplicatebehaviour,"field790"=>reserved)
}

pub fn loc(v: &LocType377) -> Value {
    let mut row = fields!(v; "field1627"=>id,"field1617"=>model,"field1643"=>shape,"field1630"=>name,
        "field1637"=>desc,"field1653"=>recol_s,"field1646"=>recol_d,"field1655"=>width,"field1629"=>length,
        "field1664"=>blockwalk,"field1663"=>blockrange,"field1613"=>active,"field1623"=>hillskew,
        "field1658"=>sharelight,"field1651"=>occlude,"field1657"=>anim,"field1656"=>wallwidth,
        "field1638"=>ambient,"field1641"=>contrast,"field1644"=>op,"field1660"=>mapfunction,
        "field1649"=>mapscene,"field1652"=>mirror,"field1661"=>shadow,"field1634"=>resizex,
        "field1614"=>resizey,"field1650"=>resizez,"field1615"=>offsetx,"field1639"=>offsety,
        "field1620"=>offsetz,"field1618"=>forceapproach,"field1619"=>forcedecor,"field1645"=>breakroutefinding,
        "field1648"=>raiseobject,"field1632"=>morph_varbit,"field1635"=>morph_varp,"field1659"=>morphs);
    row["field1637"] = signed_bytes(&v.desc);
    row
}

pub fn npc(v: &NpcType377) -> Value {
    let mut row = fields!(v; "field1431"=>id,"field1429"=>model,"field1455"=>name,"field1463"=>desc,
        "field1445"=>size,"field1424"=>readyanim,"field1448"=>walkanim,"field1446"=>walkanim_b,
        "field1444"=>walkanim_r,"field1436"=>walkanim_l,"field1449"=>op,"field1437"=>recol_s,
        "field1459"=>recol_d,"field1428"=>head,"field1451"=>field_1451,"field1430"=>field_1430,
        "field1440"=>field_1440,"field1439"=>minimap,"field1442"=>vislevel,"field1435"=>resizeh,
        "field1433"=>resizev,"field1447"=>alwaysontop,"field1466"=>ambient,"field1461"=>contrast,
        "field1441"=>headicon,"field1454"=>turnspeed,"field1434"=>field_1434,"field1457"=>morph_varbit,
        "field1462"=>morph_varp,"field1425"=>morphs);
    row["field1463"] = signed_bytes(&v.desc);
    row
}

pub fn obj(v: &ObjType377) -> Value {
    let mut row = fields!(v; "field845"=>id,"field842"=>model,"field811"=>name,"field810"=>desc,
        "field846"=>recol_s,"field823"=>recol_d,"field851"=>zoom2d,"field841"=>xan2d,"field838"=>yan2d,
        "field821"=>zan2d,"field809"=>xof2d,"field822"=>yof2d,"field853"=>stackable,"field827"=>cost,
        "field859"=>members,"field820"=>op,"field830"=>iop,"field835"=>manwear,"field813"=>manwear2,
        "field860"=>manwear_offset,"field808"=>womanwear,"field837"=>womanwear2,"field812"=>womanwear_offset,
        "field852"=>manwear3,"field849"=>womanwear3,"field816"=>manhead,"field857"=>womanhead,
        "field843"=>manhead2,"field844"=>womanhead2,"field847"=>countobj,"field858"=>countco,
        "field815"=>certlink,"field825"=>certtemplate,"field848"=>resizex,"field839"=>resizey,
        "field850"=>resizez,"ambient"=>ambient,"contrast"=>contrast,"field814"=>team,"field854"=>field_854);
    row["field810"] = signed_bytes(&v.desc);
    row
}

pub fn flo(v: &FloType377) -> Value {
    fields!(v; "field797"=>debugname,"field798"=>colour,"field799"=>texture,"field800"=>overlay,
        "field801"=>occlude,"field802"=>hue,"field803"=>saturation,"field804"=>lightness,
        "field805"=>underlay_hue,"field806"=>chroma)
}

pub fn idk(v: &IdkType377) -> Value {
    fields!(v; "field1700"=>r#type,"field1701"=>models,"field1702"=>recol_s,"field1703"=>recol_d,
        "field1704"=>heads,"field1705"=>disable)
}

pub fn spot(v: &SpotType377) -> Value {
    fields!(v; "field1298"=>index,"field1299"=>model,"field1300"=>anim,"field1302"=>recol_s,
        "field1303"=>recol_d,"field1304"=>resizeh,"field1305"=>resizev,"field1306"=>orientation,
        "field1307"=>ambient,"field1308"=>contrast)
}

pub fn varp(v: &VarpType377) -> Value {
    fields!(v; "field1510"=>debugname,"field1511"=>code1,"field1512"=>code2,"field1513"=>tracked,
        "field1514"=>code4,"field1515"=>clientcode,"field1516"=>code6,"field1517"=>code7,
        "field1518"=>scope,"field1519"=>transmit,"field1520"=>code12,"field1521"=>code14)
}

pub fn varbit(v: &VarbitType377) -> Value {
    fields!(v; "field1761"=>debugname,"field1762"=>basevar,"field1763"=>startbit,"field1764"=>endbit,
        "field1765"=>transmit,"field1766"=>code3,"field1767"=>code4,"field1768"=>code5)
}
