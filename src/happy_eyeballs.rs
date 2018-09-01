use std::mem;
use std::net::{SocketAddr, IpAddr};
use std::collections::{BTreeMap, btree_map};
use std::collections::VecDeque;
use futures::{Future, Poll, Async};
use tokio::net::{ConnectFuture, TcpStream};
use trust_dns_resolver::{IntoName, Name, ResolverFuture, error::ResolveError};
use trust_dns_resolver::lookup::SrvLookupFuture;
use trust_dns_resolver::lookup_ip::LookupIpFuture;
use trust_dns_proto::rr::rdata::srv::SRV;

pub struct Connecter {
    fallback_port: u16,
    name: Name,
    domain: Name,
    resolver_future: Box<Future<Item = ResolverFuture, Error = ResolveError> + Send>,
    resolver_opt: Option<ResolverFuture>,
    srv_lookup_opt: Option<SrvLookupFuture>,
    srvs_opt: Option<btree_map::IntoIter<u16, SRV>>,
    ip_lookup_opt: Option<(u16, LookupIpFuture)>,
    ips_opt: Option<(u16, VecDeque<IpAddr>)>,
    connect_opt: Option<ConnectFuture>,
}

impl Connecter {
    pub fn from_lookup(domain: &str, srv: &str, fallback_port: u16) -> Result<Connecter, String> {
        let resolver_future = ResolverFuture::from_system_conf()
            .map_err(|e| format!("Configure resolver: {:?}", e))?;

        let name = format!("{}.{}.", srv, domain).into_name()
            .map_err(|e| format!("Parse service name: {:?}", e))?;

        Ok(Connecter {
            fallback_port,
            name,
            domain: domain.into_name().map_err(|e| format!("Parse domain name: {:?}", e))?,
            resolver_future,
            resolver_opt: None,
            srv_lookup_opt: None,
            srvs_opt: None,
            ip_lookup_opt: None,
            ips_opt: None,
            connect_opt: None,
        })
    }
}

impl Future for Connecter {
    type Item = TcpStream;
    type Error = String;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if self.resolver_opt.is_none() {
            //println!("Poll resolver future");
            match self.resolver_future.poll() {
                Ok(Async::Ready(resolver)) => {
                    self.resolver_opt = Some(resolver);
                }
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Err(e) => return Err(format!("Cann't get resolver: {:?}", e)),
            }
        }

        if let Some(ref resolver) = self.resolver_opt {
            if self.srvs_opt.is_none() {
                if self.srv_lookup_opt.is_none() {
                    //println!("Lookup srv: {:?}", self.name);
                    self.srv_lookup_opt = Some(resolver.lookup_srv(&self.name));
                }

                if let Some(ref mut srv_lookup) = self.srv_lookup_opt {
                    match srv_lookup.poll() {
                        Ok(Async::Ready(t)) => {
                            let mut srvs = BTreeMap::new();
                            for srv in t.iter() {
                                srvs.insert(srv.priority(), srv.clone());
                            }
                            srvs.insert(65535, SRV::new(65535, 0, self.fallback_port, self.domain.clone()));
                            self.srvs_opt = Some(srvs.into_iter());
                        }
                        Ok(Async::NotReady) => return Ok(Async::NotReady),
                        Err(_) => {
                            //println!("Ignore SVR error: {:?}", e);
                            let mut srvs = BTreeMap::new();
                            srvs.insert(65535, SRV::new(65535, 0, self.fallback_port, self.domain.clone()));
                            self.srvs_opt = Some(srvs.into_iter());
                        },
                    }
                }
            }

            if self.connect_opt.is_none() {
                if self.ips_opt.is_none() {
                    if self.ip_lookup_opt.is_none() {
                        if let Some(ref mut srvs) = self.srvs_opt {
                            if let Some((_, srv)) = srvs.next() {
                                //println!("Lookup ip: {:?}", srv);
                                self.ip_lookup_opt = Some((srv.port(), resolver.lookup_ip(srv.target())));
                            } else {
                                return Err("Cann't connect".to_string());
                            }
                        }
                    }

                    if let Some((port, mut ip_lookup)) = mem::replace(&mut self.ip_lookup_opt, None) {
                        match ip_lookup.poll() {
                            Ok(Async::Ready(t)) => {
                                let mut ip_deque = VecDeque::new();
                                ip_deque.extend(t.iter());
                                //println!("IPs: {:?}", ip_deque);
                                self.ips_opt = Some((port, ip_deque));
                                self.ip_lookup_opt = None;
                            },
                            Ok(Async::NotReady) => {
                                self.ip_lookup_opt = Some((port, ip_lookup));
                                return Ok(Async::NotReady)
                            },
                            Err(_) => {
                                //println!("Ignore lookup error: {:?}", e);
                                self.ip_lookup_opt = None;
                            }
                        }
                    }
                }

                if let Some((port, mut ip_deque)) = mem::replace(&mut self.ips_opt, None) {
                    if let Some(ip) = ip_deque.pop_front() {
                        //println!("Connect to {:?}:{}", ip, port);
                        self.connect_opt = Some(TcpStream::connect(&SocketAddr::new(ip, port)));
                        self.ips_opt = Some((port, ip_deque));
                    }
                }
            }

            if let Some(mut connect_future) = mem::replace(&mut self.connect_opt, None) {
                match connect_future.poll() {
                    Ok(Async::Ready(t)) => return Ok(Async::Ready(t)),
                    Ok(Async::NotReady) => {
                        self.connect_opt = Some(connect_future);
                        return Ok(Async::NotReady)
                    }
                    Err(_) => {
                        //println!("Ignore connect error: {:?}", e);
                    },
                }
            }

        }

        Ok(Async::NotReady)
    }
}

