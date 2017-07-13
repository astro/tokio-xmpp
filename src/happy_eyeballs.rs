use std::str::FromStr;
use futures::*;
use tokio_core::reactor::Handle;
use tokio_core::net::{TcpStream, TcpStreamNew};
use domain::resolv::Resolver;
use domain::resolv::lookup::srv::*;
use domain::bits::DNameBuf;

pub struct Connecter {
    handle: Handle,
    resolver: Resolver,
    lookup: Option<LookupSrv>,
    srvs: Option<LookupSrvStream>,
    connects: Vec<TcpStreamNew>,
}

impl Connecter {
    pub fn from_lookup(handle: Handle, domain: &str, srv: &str, fallback_port: u16) -> Result<Connecter, String> {
        let domain = try!(
            DNameBuf::from_str(domain)
                .map_err(|e| format!("{}", e))
        );
        let srv = try!(
            DNameBuf::from_str(srv)
                .map_err(|e| format!("{}", e))
        );

        let resolver = Resolver::new(&handle);
        let lookup = lookup_srv(resolver.clone(), srv, domain, fallback_port);

        Ok(Connecter {
            handle,
            resolver,
            lookup: Some(lookup),
            srvs: None,
            connects: vec![],
        })
    }
}

impl Future for Connecter {
    type Item = TcpStream;
    type Error = String;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.lookup.as_mut().map(|mut lookup| lookup.poll()) {
            None => (),
            Some(Ok(Async::NotReady)) => (),
            Some(Ok(Async::Ready(found_srvs))) => {
                self.lookup = None;
                match found_srvs {
                    Some(srvs) =>
                        self.srvs = Some(srvs.to_stream(self.resolver.clone())),
                    None =>
                        return Err("No SRV records".to_owned()),
                }
            },
            Some(Err(e)) =>
                return Err(format!("{}", e)),
        }

        match self.srvs.as_mut().map(|mut srv| srv.poll()) {
            None => (),
            Some(Ok(Async::NotReady)) => (),
            Some(Ok(Async::Ready(None))) =>
                self.srvs = None,
            Some(Ok(Async::Ready(Some(srv_item)))) => {
                for addr in srv_item.to_socket_addrs() {
                    println!("Connect to {}", addr);
                    let connect = TcpStream::connect(&addr, &self.handle);
                    self.connects.push(connect);
                }
            },
            Some(Err(e)) =>
                return Err(format!("{}", e)),
        }

        for mut connect in &mut self.connects {
            match connect.poll() {
                Ok(Async::NotReady) => (),
                Ok(Async::Ready(tcp_stream)) =>
                    // Success!
                    return Ok(Async::Ready(tcp_stream)),
                Err(e) =>
                    return Err(format!("{}", e)),
            }
        }
        
        Ok(Async::NotReady)
    }
}
