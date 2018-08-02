use std::str::FromStr;
use std::collections::HashMap;
use std::net::SocketAddr;
use futures::{Future, Poll, Async, Stream};
use tokio_core::reactor::Handle;
use tokio_core::net::{TcpStream, TcpStreamNew};
use domain::resolv::Resolver;
use domain::resolv::lookup::srv::{lookup_srv, LookupSrv, LookupSrvStream};
use domain::bits::DNameBuf;

pub struct Connecter {
    handle: Handle,
    resolver: Resolver,
    lookup: Option<LookupSrv>,
    srvs: Option<LookupSrvStream>,
    connects: HashMap<SocketAddr, TcpStreamNew>,
}

impl Connecter {
    pub fn from_lookup(handle: Handle, domain: &str, srv: &str, fallback_port: u16) -> Result<Connecter, String> {
        let domain = DNameBuf::from_str(domain)
            .map_err(|e| format!("{}", e))?;
        let srv = DNameBuf::from_str(srv)
            .map_err(|e| format!("{}", e))?;

        let resolver = Resolver::new(&handle);
        let lookup = lookup_srv(resolver.clone(), srv, domain, fallback_port);

        Ok(Connecter {
            handle,
            resolver,
            lookup: Some(lookup),
            srvs: None,
            connects: HashMap::new(),
        })
    }
}

impl Future for Connecter {
    type Item = TcpStream;
    type Error = String;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.lookup.as_mut().map(|lookup| lookup.poll()) {
            None | Some(Ok(Async::NotReady)) => (),
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

        match self.srvs.as_mut().map(|srv| srv.poll()) {
            None | Some(Ok(Async::NotReady)) => (),
            Some(Ok(Async::Ready(None))) =>
                self.srvs = None,
            Some(Ok(Async::Ready(Some(srv_item)))) => {
                let handle = &self.handle;
                for addr in srv_item.to_socket_addrs() {
                    self.connects.entry(addr)
                        .or_insert_with(|| {
                            println!("Connect to {}", addr);
                            TcpStream::connect(&addr, handle)
                        });
                }
            },
            Some(Err(e)) =>
                return Err(format!("{}", e)),
        }

        let mut connected_stream = None;
        self.connects.retain(|_, connect| {
            if connected_stream.is_some() {
                return false;
            }

            match connect.poll() {
                Ok(Async::NotReady) => true,
                Ok(Async::Ready(tcp_stream)) => {
                    // Success!
                    connected_stream = Some(tcp_stream);
                    false
                },
                Err(e) => {
                    println!("{}", e);
                    false
                },
            }
        });
        if let Some(tcp_stream) = connected_stream {
            return Ok(Async::Ready(tcp_stream));
        }

        if  self.lookup.is_none() &&
            self.srvs.is_none() &&
            self.connects.is_empty()
        {
            return Err("All connection attempts failed".to_owned());
        }

        Ok(Async::NotReady)
    }
}
