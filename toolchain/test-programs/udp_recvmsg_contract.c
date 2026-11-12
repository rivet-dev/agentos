/* Bounded Linux/owned-sysroot UDP differential. No saturation or remote network. */
#include <sys/socket.h>
#include <sys/uio.h>
#include <netinet/in.h>
#include <poll.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#define CHECK(x) do { if (!(x)) { fprintf(stderr,"line %d: %s (errno=%d)\n",__LINE__,#x,errno); exit(1); } } while(0)
static void ready(int fd) { struct pollfd p={.fd=fd,.events=POLLIN}; CHECK(poll(&p,1,2000)==1 && (p.revents&POLLIN)); }
static void empty(int fd) { unsigned char b; CHECK(recv(fd,&b,1,MSG_DONTWAIT)==-1); CHECK(errno==EAGAIN || errno==EWOULDBLOCK); }
static void source(struct sockaddr_in *got, socklen_t len, struct sockaddr_in *expected) {
 CHECK(len==sizeof(*got)); CHECK(got->sin_family==AF_INET); CHECK(got->sin_port==expected->sin_port); CHECK(got->sin_addr.s_addr==expected->sin_addr.s_addr);
}
static void socket_type_contract(int fd, int expected) {
 for(socklen_t cap=0;cap<=5;cap++) {
  unsigned char guard[8]; memset(guard,0xa5,sizeof(guard)); socklen_t actual=cap;
  CHECK(getsockopt(fd,SOL_SOCKET,SO_TYPE,guard+1,&actual)==0);
  socklen_t copied=cap<sizeof(int)?cap:sizeof(int); CHECK(actual==copied);
  CHECK(memcmp(guard+1,&expected,copied)==0); CHECK(guard[0]==0xa5);
  for(unsigned i=1+copied;i<sizeof(guard);i++)CHECK(guard[i]==0xa5);
 }
}
int main(void) {
 int fd=socket(AF_INET,SOCK_DGRAM,0); CHECK(fd>=0);
 struct sockaddr_in addr={.sin_family=AF_INET,.sin_addr.s_addr=htonl(INADDR_LOOPBACK)};
 CHECK(bind(fd,(struct sockaddr *)&addr,sizeof(addr))==0); socklen_t addrlen=sizeof(addr); CHECK(getsockname(fd,(struct sockaddr *)&addr,&addrlen)==0);
 socket_type_contract(fd,SOCK_DGRAM);
 unsigned char payload[32]; for(unsigned i=0;i<32;i++)payload[i]=(unsigned char)(i+1);
 const size_t capacities[]={0,8,32};
 for(unsigned api=0;api<3;api++) for(unsigned zero=0;zero<2;zero++) for(unsigned c=0;c<3;c++) for(unsigned trunc=0;trunc<2;trunc++) for(unsigned peek=0;peek<2;peek++) {
  size_t length=zero?0:32, capacity=capacities[c]; unsigned char guard[36]; memset(guard,0xa5,sizeof(guard));
  CHECK(sendto(fd,payload,length,0,(struct sockaddr *)&addr,addrlen)==(ssize_t)length); ready(fd);
  int flags=MSG_DONTWAIT|(trunc?MSG_TRUNC:0)|(peek?MSG_PEEK:0); ssize_t n;
  struct sockaddr_in from={0}; socklen_t fromlen=sizeof(from); int output_flags=0;
  if(api==0) {
   struct iovec iov[3]={{guard+2,capacity/2},{guard+2+capacity/2,0},{guard+2+capacity/2,capacity-capacity/2}};
   struct msghdr msg={.msg_name=&from,.msg_namelen=sizeof(from),.msg_iov=iov,.msg_iovlen=3};
   n=recvmsg(fd,&msg,flags); output_flags=msg.msg_flags; fromlen=msg.msg_namelen;
   CHECK(!!(output_flags&MSG_TRUNC)==(length>capacity)); source(&from,fromlen,&addr);
  } else if(api==1) { n=recv(fd,guard+2,capacity,flags); }
  else { n=recvfrom(fd,guard+2,capacity,flags,(struct sockaddr *)&from,&fromlen); source(&from,fromlen,&addr); }
  size_t copied=length<capacity?length:capacity; CHECK(n==(ssize_t)(trunc?length:copied)); CHECK(memcmp(guard+2,payload,copied)==0);
  CHECK(guard[0]==0xa5 && guard[1]==0xa5); for(size_t i=2+copied;i<sizeof(guard);i++)CHECK(guard[i]==0xa5);
  if(peek) { unsigned char full[32]; CHECK(recv(fd,full,sizeof(full),MSG_DONTWAIT)==(ssize_t)length); CHECK(memcmp(full,payload,length)==0); }
  empty(fd); printf("api=%u length=%zu capacity=%zu trunc=%u peek=%u returned=%zd flags=%d\n",api,length,capacity,trunc,peek,n,output_flags);
 }
 CHECK(close(fd)==0);
 /* Stream fallback must not request datagram MSG_TRUNC semantics. */
 int server=socket(AF_INET,SOCK_STREAM,0); CHECK(server>=0); socket_type_contract(server,SOCK_STREAM); addr.sin_port=0;
 CHECK(bind(server,(struct sockaddr *)&addr,sizeof(addr))==0); CHECK(getsockname(server,(struct sockaddr *)&addr,&addrlen)==0); CHECK(listen(server,1)==0);
 int client=socket(AF_INET,SOCK_STREAM,0); CHECK(client>=0); CHECK(connect(client,(struct sockaddr *)&addr,addrlen)==0); int accepted=accept(server,NULL,NULL); CHECK(accepted>=0);
 CHECK(send(client,payload,8,0)==8); ready(accepted);
 unsigned char bytes[4]; struct iovec iov={bytes,sizeof(bytes)}; struct msghdr msg={.msg_iov=&iov,.msg_iovlen=1};
 CHECK(recvmsg(accepted,&msg,MSG_DONTWAIT)==4); CHECK(msg.msg_flags==0); CHECK(memcmp(bytes,payload,4)==0);
 CHECK(close(accepted)==0); CHECK(close(client)==0); CHECK(close(server)==0);
 puts("udp recvmsg contract ok"); return 0;
}
