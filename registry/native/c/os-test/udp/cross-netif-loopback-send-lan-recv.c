/* Test sending from the loopback network to the internet. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	struct sockaddr_in sin;
	memset(&sin, 0, sizeof(sin));
	sin.sin_family = AF_INET;
	sin.sin_addr.s_addr = htobe32(BLACKHOLE_HOST);
	sin.sin_port = htobe16(BLACKHOLE_PORT);
	if ( connect(fd, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "connect");
	struct sockaddr_in local;
	socklen_t locallen = sizeof(local);
	if ( getsockname(fd, (struct sockaddr*) &local, &locallen) < 0 )
		err(1, "getsockname");
	close(fd);
	int fd1 = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd1 < 0 )
		err(1, "first socket");
	struct sockaddr_in cos;
	memset(&cos, 0, sizeof(cos));
	cos.sin_family = AF_INET;
	cos.sin_addr.s_addr = htobe32(INADDR_LOOPBACK);
	cos.sin_port = htobe16(0);
	if ( bind(fd1, (const struct sockaddr*) &cos, sizeof(cos)) < 0 )
		err(1, "first bind");
	int fd2 = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd2 < 0 )
		err(1, "second socket");
	struct sockaddr_in tan;
	memset(&tan, 0, sizeof(tan));
	tan.sin_family = AF_INET;
	tan.sin_addr.s_addr = local.sin_addr.s_addr;
	tan.sin_port = htobe16(0);
	if ( bind(fd2, (const struct sockaddr*) &tan, sizeof(tan)) < 0 )
		err(1, "second bind");
	struct sockaddr_in fd2addr;
	socklen_t fd2addrlen = sizeof(fd2addr);
	if ( getsockname(fd2, (struct sockaddr*) &fd2addr, &fd2addrlen) < 0 )
		err(1, "second getsockname");
	char x = 'x';
	if ( sendto(fd1, &x, sizeof(x), 0,
	            (const struct sockaddr*) &fd2addr, sizeof(fd2addr)) < 0 )
		err(1, "sendto");
	usleep(50000);
	int errnum;
	socklen_t errnumlen = sizeof(errnum);
	if ( getsockopt(fd1, SOL_SOCKET, SO_ERROR, &errnum, &errnumlen) < 0 )
		err(1, "getsockopt: SO_ERROR");
	errno = errnum;
	if ( errnum )
		err(1, "SO_ERROR");
	struct sockaddr_in sender;
	socklen_t senderlen = sizeof(sender);
	char c;
	ssize_t amount = recvfrom(fd2, &c, sizeof(c), MSG_DONTWAIT,
	                          (struct sockaddr*) &sender, &senderlen);
	if ( amount < 0 )
		err(1, "recvfrom");
	else if ( amount == 0 )
		errx(1, "recvfrom: EOF");
	char host[INET_ADDRSTRLEN + 1];
	char port[5 + 1];
	getnameinfo((const struct sockaddr*) &sender, senderlen, host, sizeof(host),
	            port, sizeof(port), NI_NUMERICHOST | NI_NUMERICSERV);
	if ( is_on_lan(sender.sin_addr.s_addr) )
		printf("192.168.1.x");
	else
		printf("%s", host);
	printf(":");
	if ( !strcmp(port, "0") )
		printf("%s", port);
	else
		printf("non-zero");
	printf(": ");
	if ( amount != 1 )
		printf("recv %zi bytes", amount);
	else if ( c == 'x' )
		putchar(x);
	else
		printf("recv wrong byte");
	putchar('\n');
	return 0;
}
