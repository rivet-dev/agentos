#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <net/if.h>
#include <netinet/in.h>
#include <poll.h>
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static int check(int ok, const char *name) {
    printf("udp_ancillary_%s=%s\n", name, ok ? "yes" : "no");
    return ok;
}

static int transferred_alias(int fd) {
    int pair[2], received_fd = -1;
    if (socketpair(AF_UNIX, SOCK_STREAM, 0, pair)) return -1;
    char byte = 'x'; unsigned char control[CMSG_SPACE(sizeof(int))];
    memset(control, 0, sizeof(control));
    struct iovec iov = {.iov_base = &byte, .iov_len = 1};
    struct msghdr message = {.msg_iov = &iov, .msg_iovlen = 1, .msg_control = control, .msg_controllen = sizeof(control)};
    struct cmsghdr *header = CMSG_FIRSTHDR(&message);
    header->cmsg_level = SOL_SOCKET; header->cmsg_type = SCM_RIGHTS; header->cmsg_len = CMSG_LEN(sizeof(int));
    memcpy(CMSG_DATA(header), &fd, sizeof(fd));
    if (sendmsg(pair[0], &message, 0) == 1) {
        memset(control, 0, sizeof(control));
        message.msg_controllen = sizeof(control);
        if (recvmsg(pair[1], &message, 0) == 1) {
            header = CMSG_FIRSTHDR(&message);
            if (header && header->cmsg_level == SOL_SOCKET && header->cmsg_type == SCM_RIGHTS && header->cmsg_len == CMSG_LEN(sizeof(int)))
                memcpy(&received_fd, CMSG_DATA(header), sizeof(received_fd));
        }
    }
    close(pair[0]); close(pair[1]);
    return received_fd;
}

static int unbound_address_controls(int family, int kind) {
    int fd = socket(family, kind, 0);
    if (fd < 0) return check(0, "unbound_socket_create");
    int alias = dup(fd), ok = check(alias >= 0, "unbound_socket_alias");
    for (int pass = 0; pass < 3; pass++) {
        int current = pass == 1 ? alias : fd;
        struct sockaddr_storage address;
        memset(&address, 0xa5, sizeof(address));
        socklen_t length = sizeof(address);
        int result = getsockname(current, (void *)&address, &length);
        int valid = result == 0 && address.ss_family == family;
        if (valid && family == AF_INET) {
            const struct sockaddr_in *a = (const void *)&address;
            valid = length == sizeof(*a) && a->sin_port == 0 && a->sin_addr.s_addr == htonl(INADDR_ANY);
        } else if (valid) {
            const struct sockaddr_in6 *a = (const void *)&address;
            valid = length == sizeof(*a) && a->sin6_port == 0 && IN6_IS_ADDR_UNSPECIFIED(&a->sin6_addr);
        }
        ok &= check(valid, "unbound_wildcard_address");
        errno = 0;
        length = sizeof(address);
        ok &= check(getpeername(current, (void *)&address, &length) == -1 && errno == ENOTCONN,
            "unbound_peer_remains_disconnected");
    }
    if (alias >= 0) close(alias);
    close(fd);
    return ok;
}

static int option_controls(int family) {
    int fd = socket(family, SOCK_DGRAM, 0), ok = fd >= 0;
    if (fd < 0) return check(0, "option_socket");
    const int level[] = {IPPROTO_IP, IPPROTO_IP, IPPROTO_IPV6, IPPROTO_IPV6};
    const int option[] = {IP_PKTINFO, IP_RECVTOS, IPV6_RECVPKTINFO, IPV6_RECVTCLASS};
    for (int k = 0; k < 4; k++) {
        int one = 1, value = -1;
        socklen_t length = sizeof(value);
        if (family == AF_INET && k >= 2) {
            errno = 0;
            ok &= check(setsockopt(fd, level[k], option[k], &one, 4) == -1 && errno == ENOPROTOOPT, "v6_option_on_v4");
            continue;
        }
        ok &= check(setsockopt(fd, level[k], option[k], &one, 4) == 0 &&
            getsockopt(fd, level[k], option[k], &value, &length) == 0 && value == 1 && length == 4, "set_get_enable");
        int alias = dup(fd);
        value = 0;
        ok &= check(alias >= 0 && setsockopt(alias, level[k], option[k], &value, 4) == 0, "alias_disable");
        value = -1; length = 4;
        ok &= check(getsockopt(fd, level[k], option[k], &value, &length) == 0 && value == 0, "alias_shared_state");
        if (alias >= 0) close(alias);
        alias = transferred_alias(fd);
        ok &= check(alias >= 0 && setsockopt(alias, level[k], option[k], &one, 4) == 0, "transferred_alias_enable");
        value = -1; length = 4;
        ok &= check(getsockopt(fd, level[k], option[k], &value, &length) == 0 && value == 1, "transferred_alias_shared_state");
        if (alias >= 0) close(alias);
        errno = 0;
        int result = setsockopt(fd, level[k], option[k], NULL, 4);
        ok &= check(k < 2 ? result == -1 && errno == EFAULT : result == 0, "null_option");
        errno = 0;
        result = setsockopt(fd, level[k], option[k], &one, 1);
        ok &= check(k < 2 ? result == 0 : result == -1 && errno == EINVAL, "short_option");
        errno = 0;
        ok &= check(setsockopt(fd, level[k], option[k], (const void *)UINTPTR_MAX, 4) == -1 && errno == EFAULT, "invalid_option_pointer");
        struct sockaddr_storage address; socklen_t address_len = sizeof(address);
        memset(&address, 0, sizeof(address));
        ok &= check(getsockname(fd, (void *)&address, &address_len) == 0 &&
            (family == AF_INET ? ((struct sockaddr_in *)&address)->sin_port == 0 : ((struct sockaddr_in6 *)&address)->sin6_port == 0), "unbound_stays_unbound");
    }
    close(fd);
    return ok;
}

static int packet(int family, size_t capacity, int enabled_before, int enabled_after) {
    int receiver = socket(family, SOCK_DGRAM, 0), sender = socket(family, SOCK_DGRAM, 0);
    int ok = receiver >= 0 && sender >= 0, tc = 171;
    if (!ok) goto done;
    struct sockaddr_storage address; socklen_t address_len;
    memset(&address, 0, sizeof(address));
    if (family == AF_INET) {
        struct sockaddr_in *a = (void *)&address; a->sin_family = family;
        a->sin_addr.s_addr = htonl(INADDR_ANY); address_len = sizeof(*a);
    } else {
        struct sockaddr_in6 *a = (void *)&address; a->sin6_family = family;
        a->sin6_addr = in6addr_any; address_len = sizeof(*a);
    }
    const int level = family == AF_INET ? IPPROTO_IP : IPPROTO_IPV6;
    const int info = family == AF_INET ? IP_PKTINFO : IPV6_RECVPKTINFO;
    const int traffic = family == AF_INET ? IP_RECVTOS : IPV6_RECVTCLASS;
    if (bind(receiver, (void *)&address, address_len) || getsockname(receiver, (void *)&address, &address_len) ||
        setsockopt(receiver, level, info, &enabled_before, 4) || setsockopt(receiver, level, traffic, &enabled_before, 4) ||
        setsockopt(sender, level, family == AF_INET ? IP_TOS : IPV6_TCLASS, &tc, 4)) { ok = 0; goto done; }
    if (family == AF_INET) inet_pton(AF_INET, "127.0.0.2", &((struct sockaddr_in *)&address)->sin_addr);
    else ((struct sockaddr_in6 *)&address)->sin6_addr = in6addr_loopback;
    const unsigned char payload[] = {0,255,128};
    if (sendto(sender, payload, sizeof(payload), 0, (void *)&address, address_len) != 3) { ok = 0; goto done; }
    struct pollfd poll_fd = {.fd = receiver, .events = POLLIN};
    if (poll(&poll_fd, 1, 1000) != 1) { ok = 0; goto done; }
    if (!enabled_before) {
        unsigned char data[3]; struct iovec iov = {.iov_base = data, .iov_len = sizeof(data)};
        struct msghdr message = {.msg_iov = &iov, .msg_iovlen = 1};
        ok &= check(recvmsg(receiver, &message, MSG_PEEK | MSG_DONTWAIT) == 3 && message.msg_flags == 0 &&
            message.msg_controllen == 0 && memcmp(data, payload, 3) == 0, "forced_admission_before_enable");
    }
    int disabled = 0;
    if (setsockopt(receiver, level, info, &disabled, 4) || setsockopt(receiver, level, traffic, &disabled, 4) ||
        setsockopt(receiver, level, info, &enabled_after, 4) || setsockopt(receiver, level, traffic, &enabled_after, 4)) { ok = 0; goto done; }
    for (int peek = 1; peek >= 0; peek--) {
        unsigned char data[3] = {0}, control[128];
        memset(control, 0xaa, sizeof(control));
        struct iovec iov = {.iov_base = data, .iov_len = peek ? 2 : 3};
        struct msghdr message = {.msg_iov = &iov, .msg_iovlen = 1, .msg_control = control, .msg_controllen = capacity};
        ssize_t received = recvmsg(receiver, &message, MSG_DONTWAIT | (peek ? MSG_PEEK | MSG_TRUNC : 0));
        ok &= check(received == 3 && memcmp(data, payload, peek ? 2 : 3) == 0, "payload_peek_consume");
        const size_t info_data = family == AF_INET ? sizeof(struct in_pktinfo) : sizeof(struct in6_pktinfo);
        const size_t traffic_data = family == AF_INET ? 1 : sizeof(int);
        const size_t complete = CMSG_SPACE(info_data) + CMSG_LEN(traffic_data);
        const int truncated = enabled_after && capacity < complete;
        ok &= check(!!(message.msg_flags & MSG_TRUNC) == peek && !!(message.msg_flags & MSG_CTRUNC) == truncated, "independent_truncation_flags");
        ok &= check(message.msg_controllen <= capacity && (!enabled_after ? message.msg_controllen == 0 : 1), "control_length_bound");
        ok &= check(control[capacity] == 0xaa, "control_guard");
        if (enabled_after && capacity == CMSG_SPACE(info_data) + CMSG_LEN(0)) {
            struct cmsghdr *first=CMSG_FIRSTHDR(&message),*second=first?CMSG_NXTHDR(&message,first):NULL;
            ok &= check(second && second->cmsg_len==CMSG_LEN(0),"header_only_traversal");
        }
        if (enabled_after && capacity >= complete) {
            struct cmsghdr *first = CMSG_FIRSTHDR(&message);
            ok &= check(first != NULL && first->cmsg_level == level && first->cmsg_type == (family == AF_INET ? IP_PKTINFO : IPV6_PKTINFO) && first->cmsg_len == CMSG_LEN(info_data), "packet_info_header");
            if (first != NULL) {
                if (family == AF_INET) {
                    struct in_pktinfo value; memcpy(&value, CMSG_DATA(first), sizeof(value));
                    struct in_addr expected; inet_pton(AF_INET, "127.0.0.2", &expected);
                    ok &= check(value.ipi_ifindex == (enabled_before ? (int)if_nametoindex("lo") : 0) && value.ipi_addr.s_addr == expected.s_addr && value.ipi_spec_dst.s_addr == (enabled_before ? expected.s_addr : 0), "v4_destination_identity");
                } else {
                    struct in6_pktinfo value; memcpy(&value, CMSG_DATA(first), sizeof(value));
                    ok &= check(value.ipi6_ifindex == if_nametoindex("lo") && memcmp(&value.ipi6_addr, &in6addr_loopback, 16) == 0, "v6_destination_identity");
                }
                struct cmsghdr *second = CMSG_NXTHDR(&message, first);
                ok &= check(second != NULL && second->cmsg_type == (family == AF_INET ? IP_TOS : IPV6_TCLASS) && second->cmsg_len == CMSG_LEN(traffic_data), "traffic_header");
                if (second) {
                    int value = 0; memcpy(&value, CMSG_DATA(second), traffic_data);
                    ok &= check(value == tc, "traffic_value");
                }
            }
        }
    }
 done:
    if (receiver >= 0) close(receiver);
    if (sender >= 0) close(sender);
    return check(ok, "packet_case");
}

static int mapped_packet(size_t capacity, int before, int after) {
    int receiver=socket(AF_INET6,SOCK_DGRAM,0),sender=socket(AF_INET,SOCK_DGRAM,0),alias=-1,ok=1,tc=171;
    if(receiver<0 || sender<0) { ok=0; goto done; }
    const int levels[]={IPPROTO_IPV6,IPPROTO_IPV6,IPPROTO_IP,IPPROTO_IP};
    const int options[]={IPV6_RECVPKTINFO,IPV6_RECVTCLASS,IP_PKTINFO,IP_RECVTOS};
    const int bits[]={4,8,1,2};
    for(int k=0;k<4;k++) { int enabled=!!(before&bits[k]); if(setsockopt(receiver,levels[k],options[k],&enabled,4)) { ok=0; goto done; } }
    struct sockaddr_in6 bound={.sin6_family=AF_INET6,.sin6_addr=IN6ADDR_ANY_INIT};socklen_t bound_len=sizeof(bound);
    if(bind(receiver,(void *)&bound,sizeof(bound)) || getsockname(receiver,(void *)&bound,&bound_len) ||
        setsockopt(sender,IPPROTO_IP,IP_TOS,&tc,4)) { ok=0; goto done; }
    alias=transferred_alias(receiver);if(alias<0) { ok=0; goto done; }
    struct sockaddr_in target={.sin_family=AF_INET,.sin_port=bound.sin6_port};inet_pton(AF_INET,"127.0.0.2",&target.sin_addr);
    const unsigned char payload[]={0,255,128};
    if(sendto(sender,payload,3,0,(void *)&target,sizeof(target))!=3) { ok=0; goto done; }
    struct pollfd poll_fd={.fd=alias,.events=POLLIN};if(poll(&poll_fd,1,1000)!=1) { ok=0; goto done; }
    if(!before) {
        unsigned char data[3];struct iovec iov={.iov_base=data,.iov_len=3};struct msghdr message={.msg_iov=&iov,.msg_iovlen=1};
        ok &= check(recvmsg(alias,&message,MSG_PEEK|MSG_DONTWAIT)==3 && message.msg_flags==0 && message.msg_controllen==0,"mapped_forced_admission");
    }
    int disabled=0;
    for(int k=0;k<4;k++) if(setsockopt(alias,levels[k],options[k],&disabled,4) || setsockopt(alias,levels[k],options[k],&after,4)) { ok=0; goto done; }
    int observed=-1;socklen_t observed_len=4;
    ok &= check(getsockopt(receiver,IPPROTO_IP,IP_PKTINFO,&observed,&observed_len)==0 && observed==after,"mapped_alias_options");
    for(int peek=1;peek>=0;peek--) {
        unsigned char data[3]={0},control[128];memset(control,0xaa,sizeof(control));
        struct sockaddr_in6 source;struct iovec iov={.iov_base=data,.iov_len=peek?2:3};
        struct msghdr message={.msg_name=&source,.msg_namelen=sizeof(source),.msg_iov=&iov,.msg_iovlen=1,.msg_control=control,.msg_controllen=capacity};
        ok &= check(recvmsg(peek?alias:receiver,&message,MSG_DONTWAIT|(peek?MSG_PEEK|MSG_TRUNC:0))==3 &&
            memcmp(data,payload,peek?2:3)==0,"mapped_payload_alias_peek");
        const size_t complete=CMSG_SPACE(sizeof(struct in6_pktinfo))+CMSG_SPACE(sizeof(struct in_pktinfo))+CMSG_LEN(1);
        ok &= check(!!(message.msg_flags&MSG_TRUNC)==peek && !!(message.msg_flags&MSG_CTRUNC)==(after && capacity<complete) &&
            message.msg_controllen<=capacity && control[capacity]==0xaa,"mapped_truncation_bounds");
        ok &= check(message.msg_namelen==sizeof(source) && source.sin6_family==AF_INET6 && IN6_IS_ADDR_V4MAPPED(&source.sin6_addr),"mapped_source_address");
        if(after && capacity>=complete) {
            struct cmsghdr *first=CMSG_FIRSTHDR(&message),*second=first?CMSG_NXTHDR(&message,first):NULL,*third=second?CMSG_NXTHDR(&message,second):NULL;
            ok &= check(first && first->cmsg_level==IPPROTO_IPV6 && first->cmsg_type==IPV6_PKTINFO && first->cmsg_len==CMSG_LEN(sizeof(struct in6_pktinfo)),"mapped_v6_header");
            ok &= check(second && second->cmsg_level==IPPROTO_IP && second->cmsg_type==IP_PKTINFO && second->cmsg_len==CMSG_LEN(sizeof(struct in_pktinfo)),"mapped_v4_header");
            ok &= check(third && third->cmsg_level==IPPROTO_IP && third->cmsg_type==IP_TOS && third->cmsg_len==CMSG_LEN(1) && CMSG_NXTHDR(&message,third)==NULL,"mapped_tos_not_v6_tclass");
            if(first && second && third) {
                struct in6_pktinfo info6;struct in_pktinfo info4;struct in6_addr expected6;
                memcpy(&info6,CMSG_DATA(first),sizeof(info6));memcpy(&info4,CMSG_DATA(second),sizeof(info4));inet_pton(AF_INET6,"::ffff:127.0.0.2",&expected6);
                ok &= check(info6.ipi6_ifindex==((before&5)?if_nametoindex("lo"):0) && memcmp(&info6.ipi6_addr,&expected6,16)==0,"mapped_v6_values");
                ok &= check(info4.ipi_ifindex==((before&5)?(int)if_nametoindex("lo"):0) && info4.ipi_addr.s_addr==target.sin_addr.s_addr &&
                    info4.ipi_spec_dst.s_addr==((before&5)?target.sin_addr.s_addr:0) && *(unsigned char *)CMSG_DATA(third)==171,"mapped_v4_tos_values");
            }
        }
    }
 done:
    if(alias>=0)close(alias);
    if(receiver>=0)close(receiver);
    if(sender>=0)close(sender);
    return check(ok,"mapped_packet_case");
}

static int empty_and_name_edges(void) {
    int receiver=socket(AF_INET,SOCK_DGRAM,0),sender=socket(AF_INET,SOCK_DGRAM,0),ok=1,one=1;
    if (receiver < 0 || sender < 0) { ok=0; goto done; }
    struct sockaddr_in address = {.sin_family=AF_INET,.sin_addr.s_addr=htonl(INADDR_LOOPBACK)};
    socklen_t address_len=sizeof(address);
    if (bind(receiver,(void *)&address,sizeof(address)) || getsockname(receiver,(void *)&address,&address_len) ||
        setsockopt(receiver,IPPROTO_IP,IP_PKTINFO,&one,4)) { ok=0; goto done; }
    for (int empty=1;empty>=0;empty--) {
        const unsigned char payload[]={0,255,128};
        size_t length=empty?0:sizeof(payload);
        if (sendto(sender,payload,length,0,(void *)&address,sizeof(address)) != (ssize_t)length) { ok=0; goto done; }
        unsigned char control[128],name[2]={0xaa,0xaa};
        struct msghdr message={.msg_name=name,.msg_namelen=1,.msg_control=control,.msg_controllen=sizeof(control)};
        ssize_t received=recvmsg(receiver,&message,MSG_DONTWAIT);
        ok &= check(received == 0 && !!(message.msg_flags&MSG_TRUNC) == !empty &&
            !(message.msg_flags&MSG_CTRUNC),"zero_iov_and_empty_payload");
        ok &= check(message.msg_namelen == sizeof(struct sockaddr_in) && name[0] == AF_INET && name[1] == 0xaa,"short_name_full_length");
        struct cmsghdr *header=CMSG_FIRSTHDR(&message);
        ok &= check(header && header->cmsg_level==IPPROTO_IP && header->cmsg_type==IP_PKTINFO,"empty_payload_metadata");
        errno=0;
        ok &= check(recvmsg(receiver,&message,MSG_DONTWAIT) == -1 && (errno==EAGAIN || errno==EWOULDBLOCK),"zero_iov_consumes_packet");
    }
    if (sendto(sender,"abc",3,0,(void *)&address,sizeof(address)) != 3) { ok=0; goto done; }
    char data[3];struct iovec iov={.iov_base=data,.iov_len=3};
    struct msghdr message={.msg_iov=&iov,.msg_iovlen=1,.msg_control=NULL,.msg_controllen=128};
    ok &= check(recvmsg(receiver,&message,MSG_DONTWAIT)==3 && memcmp(data,"abc",3)==0 &&
        (message.msg_flags&MSG_CTRUNC) && message.msg_controllen==0,"null_control_reports_truncation");
 done:
    if (receiver >= 0) close(receiver);
    if (sender >= 0) close(sender);
    return ok;
}

int main(int argc, char **argv) {
    int identity_ok = 1;
    if (argc == 2 && strcmp(argv[1], "--guest-identity") == 0) {
        identity_ok &= check(sizeof(struct msghdr)==28 && sizeof(struct cmsghdr)==12 &&
            CMSG_SPACE(1)==16 && sizeof(struct iovec)==8 && sizeof(struct sockaddr_in)==16 &&
            sizeof(struct sockaddr_in6)==32 && AF_INET==1 && AF_INET6==2,"guest_message_abi");
        char name[IF_NAMESIZE];
        identity_ok &= check(if_nametoindex("lo") == 1 && if_nametoindex("eth0") == 2 &&
            if_indextoname(2, name) != NULL && strcmp(name, "eth0") == 0, "guest_interface_identity");
        struct if_nameindex *interfaces = if_nameindex();
        identity_ok &= check(interfaces && interfaces[0].if_index == 1 && interfaces[1].if_index == 2 &&
            interfaces[2].if_index == 0 && interfaces[2].if_name == NULL, "guest_interface_bound");
        if_freenameindex(interfaces);
        int fd=socket(AF_INET,SOCK_DGRAM,0),one=1;
        errno=0;
        identity_ok &= check(fd>=0 && setsockopt(fd,IPPROTO_IP,IP_RECVTTL,&one,4)==-1 && errno==ENOPROTOOPT,"guest_unimplemented_receive_option");
        if(fd>=0)close(fd);
    }
    int ok = identity_ok & option_controls(AF_INET) & option_controls(AF_INET6) & empty_and_name_edges();
    for (int family_index = 0; family_index < 2; family_index++) {
        int family = family_index ? AF_INET6 : AF_INET;
        ok &= unbound_address_controls(family, SOCK_STREAM);
        ok &= unbound_address_controls(family, SOCK_DGRAM);
    }
    for (int v6 = 0; v6 < 2; v6++) {
        int family = v6 ? AF_INET6 : AF_INET;
        size_t info = v6 ? sizeof(struct in6_pktinfo) : sizeof(struct in_pktinfo);
        size_t traffic = v6 ? sizeof(int) : 1;
        const size_t capacities[] = {0, sizeof(struct cmsghdr)-1, CMSG_LEN(0), CMSG_LEN(info)-1,
            CMSG_SPACE(info), CMSG_SPACE(info)+CMSG_LEN(0), CMSG_SPACE(info)+CMSG_LEN(traffic)-1,
            CMSG_SPACE(info)+CMSG_LEN(traffic), CMSG_SPACE(info)+CMSG_SPACE(traffic)};
        for (size_t i = 0; i < sizeof(capacities)/sizeof(*capacities); i++) ok &= packet(family, capacities[i], 1, 1);
        ok &= packet(family, 128-1, 0, 1);
        ok &= packet(family, 128-1, 1, 0);
    }
    const size_t mapped_complete=CMSG_SPACE(sizeof(struct in6_pktinfo))+CMSG_SPACE(sizeof(struct in_pktinfo))+CMSG_LEN(1);
    const size_t mapped_capacities[]={0,CMSG_LEN(0),CMSG_SPACE(sizeof(struct in6_pktinfo)),mapped_complete-1,mapped_complete,127};
    for(size_t i=0;i<sizeof(mapped_capacities)/sizeof(*mapped_capacities);i++) ok &= mapped_packet(mapped_capacities[i],15,1);
    ok &= mapped_packet(127,0,1);
    ok &= mapped_packet(127,1,1);
    ok &= mapped_packet(127,4,1);
    ok &= mapped_packet(127,10,1);
    ok &= mapped_packet(127,15,0);
    return ok ? 0 : 1;
}
