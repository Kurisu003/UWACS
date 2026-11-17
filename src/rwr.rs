//! RWR threat lookup for the PFP search mode (table unchanged from pfp_writer).

#[allow(dead_code)]
pub struct RwrInfo {
    pub abrv: &'static str,
    pub nato_name: &'static str,
    pub rwr_code: &'static str,
    pub rng_nm: &'static str,
    pub rng_km: &'static str,
    pub alt_ft: &'static str,
    pub alt_mtr: &'static str,
    pub flr: &'static str,
    pub chf: &'static str,
    pub ecm: &'static str,
    pub lock_time: &'static str,
    pub guidance: &'static str,
}

pub fn search_string_to_lookup_code(s: &str) -> String {
    match s {
        "2" | "SA2" => "SA-2",
        "3" | "SA3" => "SA-3",
        "5" | "SA5" => "SA-5",
        "6" | "SA6" => "SA-6",
        "8" | "SA8" => "SA-8",
        "9" | "SA9" => "SA-9",
        "10" | "SA10" | "BB" | "CS" => "SA-10",
        "11" | "SA11" | "SD" => "SA-11",
        "13" | "SA13" => "SA-13",
        "15" | "SA15" => "SA-15",
        "18" | "SA18" => "SA-18",
        "19" | "SA19" | "S6" => "SA-19",
        "24" | "SA24" => "SA-24",
        "23" | "ZSU23" => "ZSU-23",
        "234" | "ZSU234" | "A" => "ZSU-23-4",
        "572" | "ZSU572" => "ZSU-57-2",
        "GEPARD" | "GPRD" | "GE" | "GEP" => "GEPARD",
        "M163" | "163" | "M1" => "M163",
        "M1097" | "1097" | "M10" => "M1097",
        "MIM23" | "MIM" | "MIM2" | "MI" => "MIM-23",
        other => other,
    }
    .to_string()
}

macro_rules! info {
    ($abrv:expr, $nato:expr, $rwr:expr, $nm:expr, $km:expr, $ft:expr, $m:expr, $flr:expr, $chf:expr, $ecm:expr, $lock:expr, $guid:expr) => {
        RwrInfo { abrv: $abrv, nato_name: $nato, rwr_code: $rwr, rng_nm: $nm, rng_km: $km, alt_ft: $ft, alt_mtr: $m, flr: $flr, chf: $chf, ecm: $ecm, lock_time: $lock, guidance: $guid }
    };
}

pub fn rwr_info_lut(code: &str) -> RwrInfo {
    match code {
        "SA-2" => info!("SA-2", "GUIDELINE", "2", "28.00", "51,86", "82.000", "24.992", "-", "G", "G", "MED", "RDR CMD"),
        "SA-3" => info!("SA-3", "GOA", "3", "13.5", "25", "65.600", "19.994", "-", "Y", "Y", "MED", "RDR CMD"),
        "SA-5" => info!("SA-5", "Gammon", "5", "190.00", "351,88", "115.00", "35.050", "-", "R", "R", "MED", "CMD SARH"),
        "SA-6" => info!("SA-6", "GAINFUL", "6", "19.20", "35,56", "33.000", "10.058", "-", "Y", "Y", "MED", "CMD SARH"),
        "SA-8" => info!("SA-8", "GECKO", "8", "7.50", "13,89", "21.000", "6.400", "-", "G", "G", "MED", "RDR CMD"),
        "SA-9" => info!("SA-9", "GASKIN", "-", "2.50", "4,63", "12.000", "3.657", "G", "-", "-", "FST", "IR"),
        "SA-10" => info!("SA-10", "GRUMBLE", "10", "40.00", "74,08", "150.000", "45,718", "-", "R", "R", "FST", "SARH"),
        "SA-11" => info!("SA-11", "GADFLY", "11", "19.00", "35,19", "82.000", "24.992", "-", "R", "R", "MED", "SARH"),
        "SA-13" => info!("SA-13", "GOPHER", "13", "2.80", "5,19", "15.000", "4.572", "Y", "-", "G", "FST", "IR"),
        "SA-15" => info!("SA-15", "GAUNTLET", "15", "6.50", "12,04", "26.000", "7.924", "-", "R", "R", "FST", "RDR OPT"),
        "SA-18" => info!("SA-18", "GROUSE", "-", "2.50", "4,63", "12.000", "3.657", "Y", "-", "-", "FST", "IR"),
        "SA-19" => info!("SA-19", "GRINSON", "S6", "4.00", "7,41", "16.000", "4.877", "-", "-", "R", "FST", "SACLOS"),
        "SA-24" => info!("SA-24", "GRINCH", "-", "3.20", "5,93", "12.000", "3.657", "Y", "-", "-", "FST", "IR"),
        "ZSU-23-4" => info!("ZSU-23-4", "SHILKA", "A", "1.30", "2,41", "6.500", "1.981", "-", "-", "-", "FST", "RDR"),
        "ZSU-23" => info!("ZSU-23", "SERGEY", "-", "1.30", "2,41", "6.500", "1.981", "-", "-", "-", "FST", "MK1"),
        "ZSU-57-2" => info!("ZSU-57-2", "SPARKA", "-", "6.50", "12,04", "15.000", "4.572", "-", "-", "-", "FST", "MK1"),
        "GEPARD" => info!("Gepard", "", "A", "2.00", "3,7", "9.500", "2.895", "-", "-", "-", "FST", "RDR"),
        "M163" => info!("M163", "", "A", "1.40", "2,59", "4.500", "1.372", "-", "-", "-", "FST", "RDR"),
        "M1097" => info!("M1097", "", "-", "3.70", "6,85", "11.000", "3.353", "Y", "-", "-", "FST", "IR"),
        "MIM-23" => info!("MIM-23", "", "HK", "25.60", "47,41", "45.000", "13.715", "-", "Y", "Y", "MED", "SARH"),
        _ => info!("", "", "", "", "", "", "", "", "", "", "", ""),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn gepard_alias_resolves() {
        // In the old code "GEPARD" mapped to "Gepard", which the table did not match.
        let c = super::search_string_to_lookup_code("GE");
        assert_eq!(super::rwr_info_lut(&c).abrv, "Gepard");
        let _ = super::rwr_info_lut("SA-2").rng_km;
        let _ = super::rwr_info_lut("SA-2").alt_mtr;
    }
}
