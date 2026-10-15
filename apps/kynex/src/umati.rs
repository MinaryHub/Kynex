//! A real machine tool followed over umati — OPC UA for Machine Tools
//! (OPC 40501-1, on OPC 40001-1 Machinery): the machine's OPC UA server
//! names its parts as the companion specifications do, and KYNEX finds them
//! by those names from the machine's own node (as a browse path), reading
//! them about once a second over KYNEX's OPC UA client.
//!
//! What is read:
//! - `Monitoring/MachineTool/MachineryItemState/CurrentState` (Machinery):
//!   `Executing` is cutting; `NotAvailable` and `OutOfService` are the
//!   machine away.
//! - `Production/ActiveProgram/State/CurrentState`: `Running` is cutting
//!   too; each time it comes to `Ended`, a part is counted.
//! - `Production/ActiveProgram/Name`: the program.
//!
//! The standards name no door, so a machine followed over umati keeps its
//! door as it is.

use crate::mtconnect::Observation;
use crate::plc::opcua::{Datum, NodeId, Session};

/// The companion specifications' namespaces.
const MACHINE_TOOL: &str = "http://opcfoundation.org/UA/MachineTool/";
const MACHINERY: &str = "http://opcfoundation.org/UA/Machinery/";

/// What KYNEX reads of a machine, found once a session is open.
struct Found {
    item_state: Option<NodeId>,
    program_state: Option<NodeId>,
    program_name: Option<NodeId>,
}

/// A machine's session, while it is open: its nodes, and its count of
/// programs ended.
pub struct Link {
    session: Session,
    found: Found,
    machine: String,
    last_program: Option<String>,
    ended: i64,
}

impl Link {
    /// Finds the machine `machine` (a node id) by the specifications' names
    /// on `session`.
    pub fn new(mut session: Session, machine: &str) -> Result<Self, String> {
        let root = NodeId::parse(machine)?;
        // The namespaces' indices on this server: its NamespaceArray.
        let namespaces = match session.read_data(&[NodeId::Numeric(0, 2255)])?.pop() {
            Some(Ok(Datum::Texts(list))) => list,
            _ => return Err("umati: 서버의 네임스페이스 목록을 읽지 못했습니다".into()),
        };
        let index = |uri: &str| namespaces.iter().position(|n| n == uri).map(|i| i as u16);
        let mt = index(MACHINE_TOOL).ok_or("umati: 서버에 MachineTool 사양이 없습니다")?;
        let ma = index(MACHINERY);
        let item_state = ma.and_then(|ma| {
            session
                .translate(
                    &root,
                    &[
                        (mt, "Monitoring"),
                        (mt, "MachineTool"),
                        (ma, "MachineryItemState"),
                        (0, "CurrentState"),
                    ],
                )
                .ok()
        });
        let program_state = session
            .translate(
                &root,
                &[
                    (mt, "Production"),
                    (mt, "ActiveProgram"),
                    (mt, "State"),
                    (0, "CurrentState"),
                ],
            )
            .ok();
        let program_name = session
            .translate(
                &root,
                &[(mt, "Production"), (mt, "ActiveProgram"), (mt, "Name")],
            )
            .ok();
        if item_state.is_none() && program_state.is_none() {
            return Err(format!(
                "umati: {machine} 아래에서 MachineTool의 상태(MachineryItemState, ActiveProgram)를 찾지 못했습니다"
            ));
        }
        Ok(Self {
            session,
            found: Found {
                item_state,
                program_state,
                program_name,
            },
            machine: machine.to_string(),
            last_program: None,
            ended: 0,
        })
    }

    /// The machine as it is now, as an MTConnect device would say it.
    pub fn observe(&mut self) -> Result<Observation, String> {
        let wanted: Vec<(usize, NodeId)> = [
            &self.found.item_state,
            &self.found.program_state,
            &self.found.program_name,
        ]
        .into_iter()
        .enumerate()
        .filter_map(|(i, n)| Some((i, n.clone()?)))
        .collect();
        let nodes: Vec<NodeId> = wanted.iter().map(|(_, n)| n.clone()).collect();
        let values = self.session.read_data(&nodes)?;
        let mut text = [None, None, None];
        for ((i, _), v) in wanted.iter().zip(values) {
            if let Ok(Datum::Text(t)) = v {
                text[*i] = Some(t);
            }
        }
        let [item, program, name] = text;
        // A program coming to its end: a part made.
        if program.as_deref() == Some("Ended")
            && self.last_program.as_deref() != Some("Ended")
            && self.last_program.is_some()
        {
            self.ended += 1;
        }
        self.last_program = program.clone();
        let cutting = item.as_deref() == Some("Executing") || program.as_deref() == Some("Running");
        let away = matches!(item.as_deref(), Some("NotAvailable") | Some("OutOfService"));
        Ok(Observation {
            device: self.machine.clone(),
            execution: Some(if cutting { "ACTIVE" } else { "READY" }.into()),
            door: None,
            part_count: Some(self.ended),
            availability: Some(if away { "UNAVAILABLE" } else { "AVAILABLE" }.into()),
            program: name,
        })
    }

    pub fn close(self) {
        self.session.close();
    }
}
