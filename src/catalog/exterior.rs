//! Overall assembly selections and exact source matching; no STL parsing.
use super::{Bom, Catalog, Selection};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Base {
    #[default]
    None,
    Plain,
    Trolley,
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Roof {
    #[default]
    None,
    Grid,
    Table,
    Bowl,
    UnderTable,
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Board {
    #[default]
    None,
    Ikea,
    Ussa,
}
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Exterior {
    pub base: Base,
    pub backbone: bool,
    pub stationary_handle: bool,
    pub pull_handle: bool,
    pub pull_extension: f32,
    pub board: Board,
    pub roof: Roof,
    pub top_handle: bool,
    pub wall_mount: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Base,
    Feet,
    Disk,
    Tyre,
    Cover,
    Bushing,
    Backbone,
    Stationary,
    Pull,
    Lock,
    LockS,
    Knob,
    KnobCap,
    Board,
    Roof,
    TopHandle,
    Wall,
    Receiver,
}
#[derive(Clone)]
pub struct Component {
    pub role: Role,
    pub bom: Bom,
}
#[derive(Clone, Debug)]
pub struct Hardware {
    pub name: String,
    pub qty: u32,
    pub note: &'static str,
}
impl Hardware {
    pub fn line(&self) -> String {
        if self.qty == 0 {
            format!("{} · {}", self.name, self.note)
        } else {
            format!("{} × {} · {}", self.name, self.qty, self.note)
        }
    }
}
impl Exterior {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
    pub fn validate(&self, s: &Selection, is_box: bool) -> Result<(), String> {
        if !self.pull_extension.is_finite() || !(0. ..=800.).contains(&self.pull_extension) {
            return Err("拉杆伸出量应为 0–800 mm / Invalid handle extension".into());
        }
        if (self.stationary_handle || self.pull_handle || self.board != Board::None)
            && !self.backbone
        {
            return Err(
                "把手和挂板需要背部连接板 / Handles and pegboard require a backbone".into(),
            );
        }
        if self.pull_handle && (self.stationary_handle || self.board != Board::None) {
            return Err("拉杆锁座与固定把手、挂板需分别安装到不同成员 / Mount pull locks and handle/pegboard on separate members".into());
        }
        if self.wall_mount && (self.backbone || self.base != Base::None) {
            return Err("壁挂与底座、背部连接板占用相同安装区域 / Wall mounting conflicts with base/backbone".into());
        }
        if self.board != Board::None && self.stationary_handle {
            return Err(
                "同一连接板不能同时安装固定把手和挂板 / Handle and pegboard share mounting holes"
                    .into(),
            );
        }
        if self.top_handle && self.roof != Roof::Grid {
            return Err("顶部提手需要 Grid 顶盖 / Top handle requires a Grid roof".into());
        }
        if is_box && self.roof != Roof::None {
            return Err(
                "盒子已有铰接顶盖，不能安装抽屉独立顶盖 / Box already has a hinged lid".into(),
            );
        }
        if self.backbone && s.height == "4F" {
            return Err("4F 背板跨层孔位尚未校准；请选择 1F 或 2F 安装对象 / 4F backbone attachment unavailable".into());
        }
        if self.pull_handle && self.roof == Roof::UnderTable {
            return Err(
                "桌下安装不能使用伸缩拉杆 / Pull handle conflicts with under-table mounting".into(),
            );
        }
        Ok(())
    }
    pub fn hardware(&self, s: &Selection) -> Vec<Hardware> {
        let mut rows = Vec::new();
        let mut add = |name: String, qty, note| rows.push(Hardware { name, qty, note });
        if self.base == Base::Trolley {
            let length = match s.width.as_str() {
                "S" => 248,
                "M" => 338,
                "L" => 414,
                _ => 504,
            };
            add(format!("M16 × {length} mm 轮轴 / axle"), 1, "");
            add("6205ZZ 轴承 / bearing · 25×52×15 mm".into(), 2, "");
            add("SCSAW 止动环 / shaft collar · 16 mm".into(), 2, "");
            add("M4 × 25 mm".into(), 12, "轮盖 / wheel covers");
            add("M6 × 90 mm".into(), 2, "脚撑 / foot");
        }
        if self.backbone {
            add(
                "M4 × 60–100 mm".into(),
                4,
                "长度和孔数按实际连接位置确认 / verify mounting holes and length",
            );
        }
        if self.stationary_handle {
            add("M4 × 60 mm".into(), 8, "固定把手 / stationary handle");
        }
        if self.pull_handle {
            add(
                "M16 拉杆 / rods".into(),
                2,
                "按塔高裁切 / cut to tower height",
            );
            add("M6 × 30 mm".into(), 4, "拉杆提手 / pull handle");
            add("M6 × 8 × 8 热熔嵌件 / inserts".into(), 2, "");
            add(
                "35 mm 锁紧螺栓 / lock bolts".into(),
                2,
                "文档 M4 / M6 标注冲突，待核对 / conflicting M4/M6 specification",
            );
        }
        if self.board != Board::None {
            add("M4 × 40 mm".into(), 4, "挂板 / pegboard");
        }
        if self.top_handle {
            add("M6 × 50 mm".into(), 2, "顶部提手 / top handle");
        }
        if self.wall_mount || self.roof == Roof::UnderTable {
            add(
                "安装螺钉 / mounting screws".into(),
                0,
                "按墙面或桌板材质及孔位选择 / select for substrate and holes",
            );
        }
        rows
    }
}
impl Catalog {
    pub fn exterior_components(
        &self,
        s: &Selection,
        config: &Exterior,
        is_box: bool,
    ) -> Result<Vec<Component>, String> {
        config.validate(s, is_box)?;
        let mut result = Vec::new();
        let mut add = |role, expected: String, qty| -> Result<(), String> {
            let mut matches = self
                .entries
                .iter()
                .filter(|e| e.path.eq_ignore_ascii_case(&expected));
            let found = matches
                .next()
                .ok_or_else(|| format!("缺少整体配件 / Missing exterior component: {expected}"))?;
            if matches.next().is_some() {
                return Err(format!("配件文件不唯一 / Ambiguous component: {expected}"));
            }
            result.push(Component {
                role,
                bom: Bom {
                    path: found.path.clone(),
                    qty,
                    group: "Overall accessories".into(),
                    enabled: true,
                },
            });
            Ok(())
        };
        let w = &s.width;
        let size = format!("{}{}", w, s.depth);
        let mods = "03. MODIFICATIONS";
        if config.base != Base::None {
            let whole = if s.depth >= 6 { " Whole" } else { "" };
            add(
                Role::Base,
                format!("{mods}/Base/{w}/{size} Base{whole}.stl"),
                1,
            )?;
        }
        if config.base == Base::Trolley {
            add(Role::Feet, format!("{mods}/Feet/{w} Feet.stl"), 1)?;
            for (role, name) in [
                (Role::Disk, "Disk"),
                (Role::Tyre, "Tyre (TPU)"),
                (Role::Cover, "Cover"),
                (Role::Bushing, "Bushing"),
            ] {
                add(role, format!("{mods}/Wheels/{name}.stl"), 2)?;
            }
        }
        if config.backbone && !config.pull_handle {
            add(
                Role::Backbone,
                format!("{mods}/Backbone/{w}{} Backbone.stl", s.height),
                1,
            )?;
        }
        if config.stationary_handle {
            add(
                Role::Stationary,
                format!("{mods}/Stationary Handle/{w} Stationary Handle.stl"),
                1,
            )?;
        }
        if config.pull_handle {
            add(
                Role::Pull,
                format!("{mods}/Pull-out Handle/{w} Pull-out Handle.stl"),
                1,
            )?;
            add(
                Role::Lock,
                format!("{mods}/Backbone/{w} Pull-out Handle Lock.stl"),
                1,
            )?;
            if w == "S" {
                add(
                    Role::LockS,
                    format!("{mods}/Backbone/S Pull-out Handle Lock Part 2.stl"),
                    1,
                )?;
            }
            add(
                Role::Knob,
                format!("{mods}/Backbone/Handle Lock Knob Part 1.stl"),
                2,
            )?;
            add(
                Role::KnobCap,
                format!("{mods}/Backbone/Handle Lock Knob Part 2.stl"),
                2,
            )?;
        }
        if config.board != Board::None {
            let kind = if config.board == Board::Ikea {
                "IKEA Skadis Pegboard"
            } else {
                "USSA Pegboard"
            };
            add(
                Role::Board,
                format!("{mods}/Pegboard Attachment/{kind}/{w}{}.stl", s.height),
                1,
            )?;
        }
        if config.roof != Roof::None {
            let kind = match config.roof {
                Roof::Grid => "Grid",
                Roof::Table => "Table",
                Roof::Bowl => "Bowl",
                Roof::UnderTable => "Under Table Mount",
                Roof::None => unreachable!(),
            };
            add(
                Role::Roof,
                format!("{}/Roofs/{size} {kind} Roof.stl", Self::prefix(s)),
                1,
            )?;
        }
        if config.top_handle {
            add(
                Role::TopHandle,
                "01. DRAWERS/07. Universal/Top Handle (Turn On Supports).stl".into(),
                1,
            )?;
        }
        if config.wall_mount {
            add(
                Role::Wall,
                format!("{mods}/Wall Mounts/{w}{} Wall Mount.stl", s.height),
                1,
            )?;
            add(
                Role::Receiver,
                format!("{mods}/Wall Mounts/Wall Mount Second Part.stl"),
                2,
            )?;
        }
        Ok(result)
    }
}
