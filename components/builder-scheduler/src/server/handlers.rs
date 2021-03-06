// Copyright (c) 2016-2017 Chef Software Inc. and/or applicable contributors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! A collection of handlers for the Scheduler dispatcher

use hab_net::server::Envelope;
use protocol::scheduler as proto;
use zmq;

use protocol::jobsrv::{Job, JobSpec};
use hab_net::routing::Broker;
use protocol::originsrv::*;

use super::ServerState;
use error::Result;

// TBD: This is currently a WIP to get end-to-end flow working.
pub fn schedule(req: &mut Envelope, sock: &mut zmq::Socket, state: &mut ServerState) -> Result<()> {
    let msg: proto::Schedule = try!(req.parse_msg());
    println!("Scheduling: {:?}", msg);

    let mut group = proto::Group::new();

    {
        let mut ds = state.datastore.write().unwrap();
        ds.create_group(&mut group)?;
    }

    let mut project_get = OriginProjectGet::new();
    let project_name = format!("{}/{}",
                               msg.get_ident().get_origin(),
                               msg.get_ident().get_name());
    project_get.set_name(project_name.clone());

    let mut conn = Broker::connect().unwrap();
    let project = match conn.route::<OriginProjectGet, OriginProject>(&project_get) {
        Ok(project) => project,
        Err(err) => {
            error!("Unable to retrieve project: {:?}, error: {:?}",
                   project_name,
                   err);
            group.set_state(proto::GroupState::Failed);
            {
                let mut ds = state.datastore.write().unwrap();
                ds.set_group_state(&group)?;
            }

            try!(req.reply_complete(sock, &group));
            return Ok(());
        }
    };

    let mut job_spec: JobSpec = JobSpec::new();
    job_spec.set_owner_id(group.get_group_id());
    job_spec.set_project(project);

    match conn.route::<JobSpec, Job>(&job_spec) {
        Ok(job) => {
            println!("Job created: {:?}", job);
            group.set_state(proto::GroupState::Dispatched);

            let mut ds = state.datastore.write().unwrap();
            ds.add_group_job(&group, &job)?;
            ds.set_group_state(&group)?;
        }
        Err(err) => {
            error!("Job creation error: {:?}", err);
            group.set_state(proto::GroupState::Failed);

            let mut ds = state.datastore.write().unwrap();
            ds.set_group_state(&group)?;
        }
    }

    try!(req.reply_complete(sock, &group));
    Ok(())
}
