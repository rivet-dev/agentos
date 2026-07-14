use super::super::*;

impl<B> NativeSidecar<B>
where
    B: NativeSidecarBridge + Send + 'static,
    BridgeError<B>: fmt::Debug + Send + Sync + 'static,
{
    pub(crate) async fn handle_python_vfs_rpc_request(
        &mut self,
        vm_id: &str,
        process_id: &str,
        request: PythonVfsRpcRequest,
    ) -> Result<(), SidecarError> {
        match request.method {
            PythonVfsRpcMethod::Read
            | PythonVfsRpcMethod::Write
            | PythonVfsRpcMethod::Stat
            | PythonVfsRpcMethod::Lstat
            | PythonVfsRpcMethod::ReadDir
            | PythonVfsRpcMethod::Mkdir
            | PythonVfsRpcMethod::Unlink
            | PythonVfsRpcMethod::Rmdir
            | PythonVfsRpcMethod::Rename
            | PythonVfsRpcMethod::Symlink
            | PythonVfsRpcMethod::ReadLink
            | PythonVfsRpcMethod::Setattr => {
                filesystem_handle_python_vfs_rpc_request(self, vm_id, process_id, request)
            }
            PythonVfsRpcMethod::HttpRequest => {
                self.handle_python_http_rpc_request(vm_id, process_id, request)
            }
            PythonVfsRpcMethod::DnsLookup => {
                self.handle_python_dns_rpc_request(vm_id, process_id, request)
            }
            PythonVfsRpcMethod::SubprocessRun => {
                self.handle_python_subprocess_rpc_request(vm_id, process_id, request)
                    .await
            }
            PythonVfsRpcMethod::SocketConnect
            | PythonVfsRpcMethod::SocketSend
            | PythonVfsRpcMethod::SocketRecv
            | PythonVfsRpcMethod::SocketClose
            | PythonVfsRpcMethod::UdpCreate
            | PythonVfsRpcMethod::UdpSendto
            | PythonVfsRpcMethod::UdpRecvfrom => {
                self.handle_python_socket_rpc_request(vm_id, process_id, request)
                    .await
            }
        }
    }

    fn handle_python_http_rpc_request(
        &mut self,
        vm_id: &str,
        process_id: &str,
        request: PythonVfsRpcRequest,
    ) -> Result<(), SidecarError> {
        let Some(vm) = self.vms.get_mut(vm_id) else {
            return Ok(());
        };
        if !vm.active_processes.contains_key(process_id) {
            return Ok(());
        }
        let response = (|| {
            let url_text = request.url.as_deref().ok_or_else(|| {
                SidecarError::InvalidState(String::from("python httpRequest requires a url"))
            })?;
            let url = Url::parse(url_text)
                .map_err(|error| SidecarError::Execution(format!("ERR_INVALID_URL: {error}")))?;
            let host = url.host_str().ok_or_else(|| {
                SidecarError::Execution(String::from("ERR_INVALID_URL: missing host"))
            })?;
            let port = url.port_or_known_default().ok_or_else(|| {
                SidecarError::Execution(String::from("ERR_INVALID_URL: missing port"))
            })?;
            self.bridge.require_network_access(
                vm_id,
                NetworkOperation::Http,
                format_tcp_resource(host, port),
            )?;
            // Pin the outbound connection to the IP addresses that pass the
            // egress range guard at resolution time. A literal IP is validated
            // directly; a hostname is resolved once here and the resulting
            // address set is pinned into the HTTP client's resolver below so a
            // rebinding DNS server cannot make the second (TLS/TCP) lookup land
            // on a private/link-local/metadata IP that this check rejected.
            let pinned_addresses = if let Ok(literal_ip) = host.parse::<IpAddr>() {
                filter_dns_safe_ip_addrs(vec![literal_ip], host)?
            } else {
                filter_dns_safe_ip_addrs(
                    resolve_dns_ip_addrs(
                        &self.bridge,
                        &vm.kernel,
                        vm_id,
                        &vm.dns,
                        host,
                        DnsLookupPolicy::SkipPermissions,
                    )?,
                    host,
                )?
            };
            self.bridge.require_resolved_network_access(
                vm_id,
                NetworkOperation::Http,
                &format_tcp_resource(host, port),
                &pinned_addresses
                    .iter()
                    .map(|ip| format_tcp_resource(&ip.to_string(), port))
                    .collect::<Vec<_>>(),
            )?;
            let mut headers = BTreeMap::new();
            for (name, value) in &request.headers {
                headers.insert(name.clone(), Value::String(value.clone()));
            }
            let options = JavascriptHttpRequestOptions {
                method: Some(
                    request
                        .http_method
                        .clone()
                        .unwrap_or_else(|| String::from("GET")),
                ),
                headers,
                body: request.body_base64.as_deref().map(|body| {
                    String::from_utf8(
                        base64::engine::general_purpose::STANDARD
                            .decode(body)
                            .unwrap_or_default(),
                    )
                    .unwrap_or_default()
                }),
                reject_unauthorized: None,
            };
            let headers =
                parse_http_header_collection(&options.headers, "python httpRequest headers")?;
            let default_ca_bundle = if url.scheme() == "https" {
                read_vm_default_ca_bundle(&mut vm.kernel)?
            } else {
                Vec::new()
            };
            let response = issue_outbound_http_request(
                &url,
                &options,
                &headers,
                &pinned_addresses,
                &default_ca_bundle,
            )?;
            let payload_json = response.as_str().ok_or_else(|| {
                SidecarError::Execution(String::from(
                    "python httpRequest returned a non-string response payload",
                ))
            })?;
            let payload: Value = serde_json::from_str(payload_json).map_err(|error| {
                SidecarError::Execution(format!(
                    "python httpRequest response must be valid JSON: {error}"
                ))
            })?;
            let header_map = payload
                .get("headers")
                .and_then(Value::as_array)
                .map(|entries| {
                    let mut normalized = BTreeMap::<String, Vec<String>>::new();
                    for entry in entries {
                        let Some(pair) = entry.as_array() else {
                            continue;
                        };
                        let Some(name) = pair.first().and_then(Value::as_str) else {
                            continue;
                        };
                        let Some(value) = pair.get(1).and_then(Value::as_str) else {
                            continue;
                        };
                        normalized
                            .entry(name.to_owned())
                            .or_default()
                            .push(value.to_owned());
                    }
                    normalized
                })
                .unwrap_or_default();
            Ok(PythonVfsRpcResponsePayload::Http {
                status: payload
                    .get("status")
                    .and_then(Value::as_u64)
                    .map(|value| value as u16)
                    .unwrap_or_default(),
                reason: payload
                    .get("statusText")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                url: payload
                    .get("url")
                    .and_then(Value::as_str)
                    .unwrap_or(url_text)
                    .to_owned(),
                headers: header_map,
                body_base64: payload
                    .get("body")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            })
        })();

        self.respond_python_rpc(vm_id, process_id, request.id, response)
    }

    fn handle_python_dns_rpc_request(
        &mut self,
        vm_id: &str,
        process_id: &str,
        request: PythonVfsRpcRequest,
    ) -> Result<(), SidecarError> {
        let Some(vm) = self.vms.get(vm_id) else {
            return Ok(());
        };
        if !vm.active_processes.contains_key(process_id) {
            return Ok(());
        }
        let response = (|| {
            let hostname = request.hostname.as_deref().ok_or_else(|| {
                SidecarError::InvalidState(String::from("python dnsLookup requires a hostname"))
            })?;
            let mut addresses = filter_dns_safe_ip_addrs(
                resolve_dns_ip_addrs(
                    &self.bridge,
                    &vm.kernel,
                    vm_id,
                    &vm.dns,
                    hostname,
                    DnsLookupPolicy::CheckPermissions,
                )?,
                hostname,
            )?;
            if let Some(family) = request.family {
                addresses.retain(|address| {
                    matches!((family, address), (4, IpAddr::V4(_)) | (6, IpAddr::V6(_)))
                });
            }
            Ok(PythonVfsRpcResponsePayload::DnsLookup {
                addresses: addresses
                    .into_iter()
                    .map(|address| address.to_string())
                    .collect(),
            })
        })();

        self.respond_python_rpc(vm_id, process_id, request.id, response)
    }
}
