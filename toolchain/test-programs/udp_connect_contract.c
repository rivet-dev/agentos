#define _POSIX_C_SOURCE 200809L
#include <sys/socket.h>
#include <sys/time.h>
#include <arpa/inet.h>
#include <unistd.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define CHECK(x) do { if (!(x)) { fprintf(stderr,"line %d: %s errno=%d\n",__LINE__,#x,errno); exit(1); } } while(0)
static int endpoint(struct sockaddr_in *a) {
 int fd=socket(AF_INET,SOCK_DGRAM,0);CHECK(fd>=0);
 *a=(struct sockaddr_in){.sin_family=AF_INET,.sin_addr.s_addr=htonl(INADDR_LOOPBACK)};
 CHECK(bind(fd,(struct sockaddr*)a,sizeof(*a))==0);socklen_t n=sizeof(*a);CHECK(getsockname(fd,(struct sockaddr*)a,&n)==0);
 struct timeval t={.tv_usec=200000};CHECK(setsockopt(fd,SOL_SOCKET,SO_RCVTIMEO,&t,sizeof(t))==0);return fd;
}
static void receive(int fd,char want) {char c=0;CHECK(recv(fd,&c,1,0)==1);CHECK(c==want);}
// SCM_RIGHTS aliases must share the UDP peer and remain valid after close.
static void shared_description(void) {
 struct sockaddr_in ca,aa,ba,peer;int c=endpoint(&ca),a=endpoint(&aa),b=endpoint(&ba);
 int pair[2];CHECK(socketpair(AF_UNIX,SOCK_STREAM,0,pair)==0);
 union { struct cmsghdr align; char bytes[CMSG_SPACE(sizeof(int))]; } control;
 memset(&control,0,sizeof(control));char marker='x';struct iovec iov={.iov_base=&marker,.iov_len=1};
 struct msghdr msg={.msg_iov=&iov,.msg_iovlen=1,.msg_control=control.bytes,.msg_controllen=sizeof(control.bytes)};
 struct cmsghdr *h=CMSG_FIRSTHDR(&msg);CHECK(h!=NULL);h->cmsg_level=SOL_SOCKET;h->cmsg_type=SCM_RIGHTS;h->cmsg_len=CMSG_LEN(sizeof(int));memcpy(CMSG_DATA(h),&c,sizeof(c));
 CHECK(sendmsg(pair[0],&msg,0)==1);memset(&control,0,sizeof(control));msg.msg_controllen=sizeof(control.bytes);CHECK(recvmsg(pair[1],&msg,0)==1);CHECK(!(msg.msg_flags&MSG_CTRUNC));h=CMSG_FIRSTHDR(&msg);CHECK(h && h->cmsg_level==SOL_SOCKET && h->cmsg_type==SCM_RIGHTS && h->cmsg_len==CMSG_LEN(sizeof(int)));int alias=-1;memcpy(&alias,CMSG_DATA(h),sizeof(alias));CHECK(alias>=0 && alias!=c);
 CHECK(connect(c,(struct sockaddr*)&aa,sizeof(aa))==0);socklen_t n=sizeof(peer);CHECK(getpeername(alias,(struct sockaddr*)&peer,&n)==0);CHECK(peer.sin_port==aa.sin_port);CHECK(send(alias,"a",1,0)==1);receive(a,'a');
 CHECK(connect(alias,(struct sockaddr*)&ba,sizeof(ba))==0);n=sizeof(peer);CHECK(getpeername(c,(struct sockaddr*)&peer,&n)==0);CHECK(peer.sin_port==ba.sin_port);CHECK(send(c,"b",1,0)==1);receive(b,'b');
 CHECK(close(c)==0);CHECK(send(alias,"c",1,0)==1);receive(b,'c');
 struct sockaddr unspec={.sa_family=AF_UNSPEC};CHECK(connect(alias,&unspec,sizeof(unspec))==0);CHECK(send(alias,"x",1,0)==-1);CHECK(errno==EDESTADDRREQ);
 CHECK(sendto(alias,"d",1,0,(struct sockaddr*)&aa,sizeof(aa))==1);receive(a,'d');
 CHECK(close(alias)==0);CHECK(close(pair[0])==0);CHECK(close(pair[1])==0);CHECK(close(a)==0);CHECK(close(b)==0);
 puts("PASS udp SCM_RIGHTS shared peer/reconnect/close/disconnect");
}

int main(void) {
 struct sockaddr_in ca,aa,ba,peer;int c=endpoint(&ca),a=endpoint(&aa),b=endpoint(&ba);socklen_t n=sizeof(peer);
 CHECK(connect(c,(struct sockaddr*)&aa,sizeof(aa))==0);CHECK(getpeername(c,(struct sockaddr*)&peer,&n)==0);CHECK(peer.sin_port==aa.sin_port);
 CHECK(send(c,"a",1,0)==1);receive(a,'a');
 CHECK(sendto(c,"b",1,0,(struct sockaddr*)&ba,sizeof(ba))==1);receive(b,'b');
 n=sizeof(peer);CHECK(getpeername(c,(struct sockaddr*)&peer,&n)==0);CHECK(peer.sin_port==aa.sin_port);
 CHECK(connect(c,(struct sockaddr*)&ba,sizeof(ba))==0);CHECK(send(c,"c",1,0)==1);receive(b,'c');
 CHECK(sendto(a,"x",1,0,(struct sockaddr*)&ca,sizeof(ca))==1);
 CHECK(sendto(b,"y",1,0,(struct sockaddr*)&ca,sizeof(ca))==1);receive(c,'y');
 char byte;CHECK(recv(c,&byte,1,MSG_DONTWAIT)==-1);CHECK(errno==EAGAIN||errno==EWOULDBLOCK);
 struct sockaddr unspec={.sa_family=AF_UNSPEC};CHECK(connect(c,&unspec,sizeof(unspec))==0);CHECK(connect(c,&unspec,sizeof(unspec))==0);
 n=sizeof(peer);CHECK(getsockname(c,(struct sockaddr*)&peer,&n)==0);CHECK(peer.sin_port==0);
 n=sizeof(peer);CHECK(getpeername(c,(struct sockaddr*)&peer,&n)==-1);CHECK(errno==ENOTCONN);
 CHECK(send(c,"z",1,0)==-1);CHECK(errno==EDESTADDRREQ);
 CHECK(sendto(c,"d",1,0,(struct sockaddr*)&aa,sizeof(aa))==1);receive(a,'d');
 n=sizeof(ca);CHECK(getsockname(c,(struct sockaddr*)&ca,&n)==0);
 CHECK(sendto(a,"e",1,0,(struct sockaddr*)&ca,sizeof(ca))==1);receive(c,'e');
 CHECK(sendto(b,"f",1,0,(struct sockaddr*)&ca,sizeof(ca))==1);receive(c,'f');
 CHECK(close(a)==0);a=socket(AF_INET,SOCK_DGRAM,0);CHECK(a>=0);CHECK(bind(a,(struct sockaddr*)&aa,sizeof(aa))==0);
 CHECK(connect(a,(struct sockaddr*)&ba,sizeof(ba))==0);CHECK(connect(a,&unspec,sizeof(unspec))==0);n=sizeof(peer);CHECK(getsockname(a,(struct sockaddr*)&peer,&n)==0);CHECK(peer.sin_port==aa.sin_port);
 CHECK(close(c)==0);CHECK(close(a)==0);CHECK(close(b)==0);puts("PASS udp connect peer/reconnect/sendto/filter/disconnect/default-send");shared_description();return 0;
}
