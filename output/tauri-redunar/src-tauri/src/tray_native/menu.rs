//! A static bounded host-rendered Open/Quit menu; no shell commands or paths.
use super::Action;
use gtk::glib::{variant::ToVariant, Variant};
use std::collections::BTreeMap;

type Properties = BTreeMap<String, Variant>;
const IDS: [i32; 3] = [0, 1, 2];

fn properties(id: i32, names: &[String]) -> Result<Properties, &'static str> {
    let mut values: BTreeMap<&str, Variant> = match id {
        0 => [("children-display", "submenu".to_variant())]
            .into_iter()
            .collect(),
        1 => [
            ("label", "Open Redunar".to_variant()),
            ("enabled", true.to_variant()),
            ("visible", true.to_variant()),
        ]
        .into_iter()
        .collect(),
        2 => [
            ("label", "Quit Redunar".to_variant()),
            ("enabled", true.to_variant()),
            ("visible", true.to_variant()),
        ]
        .into_iter()
        .collect(),
        _ => return Err("Unknown menu item"),
    };
    if !names.is_empty() {
        values.retain(|name, _| names.iter().any(|requested| requested == name));
    }
    Ok(values
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect())
}

fn names_valid(names: &[String]) -> bool {
    names.len() <= 16 && names.iter().all(|name| name.len() <= 64)
}

fn layout(id: i32, depth: i32, names: &[String]) -> Result<Variant, &'static str> {
    let children = if id == 0 && depth != 0 {
        vec![layout(1, 0, names)?, layout(2, 0, names)?]
    } else {
        Vec::new()
    };
    Ok((id, properties(id, names)?, children).to_variant())
}

fn event(id: i32, name: &str) -> Result<Option<Action>, &'static str> {
    if !IDS.contains(&id) || name.len() > 64 {
        return Err("Invalid menu event");
    }
    Ok(match (id, name) {
        (1, "clicked") => Some(Action::Open),
        (2, "clicked") => Some(Action::Quit),
        _ => None,
    })
}

pub(super) fn call(
    method: &str,
    parameters: &Variant,
    action: &dyn Fn(Action),
) -> Result<Variant, &'static str> {
    match method {
        "GetLayout" => {
            let (id, depth, names) = parameters
                .get::<(i32, i32, Vec<String>)>()
                .ok_or("Invalid layout request")?;
            if !names_valid(&names) || depth < -1 {
                return Err("Invalid layout request");
            }
            Ok(Variant::tuple_from_iter([
                1_u32.to_variant(),
                layout(id, depth, &names)?,
            ]))
        }
        "GetGroupProperties" => {
            let (mut ids, names) = parameters
                .get::<(Vec<i32>, Vec<String>)>()
                .ok_or("Invalid group request")?;
            if ids.len() > 16 || !names_valid(&names) {
                return Err("Invalid group request");
            }
            if ids.is_empty() {
                ids = IDS.to_vec();
            }
            let items = ids
                .into_iter()
                .map(|id| Ok((id, properties(id, &names)?)))
                .collect::<Result<Vec<_>, &'static str>>()?;
            Ok((items,).to_variant())
        }
        "GetProperty" => {
            let (id, name) = parameters
                .get::<(i32, String)>()
                .ok_or("Invalid property request")?;
            if name.len() > 64 {
                return Err("Invalid property request");
            }
            let value = properties(id, &[])?
                .remove(&name)
                .ok_or("Unknown menu property")?;
            Ok((value,).to_variant())
        }
        "Event" => {
            let (id, name, _, _) = parameters
                .get::<(i32, String, Variant, u32)>()
                .ok_or("Invalid event request")?;
            if let Some(value) = event(id, &name)? {
                action(value);
            }
            Ok(().to_variant())
        }
        "EventGroup" => {
            let (events,) = parameters
                .get::<(Vec<(i32, String, Variant, u32)>,)>()
                .ok_or("Invalid event group")?;
            if events.len() > 16 {
                return Err("Too many menu events");
            }
            let mut invalid = Vec::new();
            for (id, name, _, _) in events {
                match event(id, &name) {
                    Ok(Some(value)) => action(value),
                    Ok(None) => {}
                    Err(_) => invalid.push(id),
                }
            }
            Ok((invalid,).to_variant())
        }
        "AboutToShow" => {
            let (id,) = parameters.get::<(i32,)>().ok_or("Invalid menu request")?;
            if !IDS.contains(&id) {
                return Err("Unknown menu item");
            }
            Ok((false,).to_variant())
        }
        "AboutToShowGroup" => {
            let (ids,) = parameters
                .get::<(Vec<i32>,)>()
                .ok_or("Invalid menu group")?;
            if ids.len() > 16 {
                return Err("Too many menu items");
            }
            let invalid = ids
                .into_iter()
                .filter(|id| !IDS.contains(id))
                .collect::<Vec<_>>();
            Ok((Vec::<i32>::new(), invalid).to_variant())
        }
        _ => Err("Unknown menu method"),
    }
}

pub(super) fn property(name: &str) -> Variant {
    match name {
        "Version" => 3_u32.to_variant(),
        "TextDirection" => "ltr".to_variant(),
        "Status" => "normal".to_variant(),
        "IconThemePath" => Vec::<String>::new().to_variant(),
        _ => "".to_variant(),
    }
}
