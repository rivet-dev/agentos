/* Create three sockets on the loopback address, connect the second socket to
   the first socket, connect the third socket to the first socket, send 'y' on
   the third socket to the first socket, send 'x' on the second socket to the
   first socket, connect the first socket to the second socket, and then test
   the poll bits on the first socket. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "first socket");
	struct sockaddr_in sin;
	memset(&sin, 0, sizeof(sin));
	sin.sin_family = AF_INET;
	sin.sin_addr.s_addr = htobe32(INADDR_LOOPBACK);
	sin.sin_port = htobe16(0);
	if ( bind(fd, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "first bind");
	struct sockaddr_in local;
	socklen_t locallen = sizeof(local);
	if ( getsockname(fd, (struct sockaddr*) &local, &locallen) < 0 )
		err(1, "first getsockname");
	int fd2 = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd2 < 0 )
		err(1, "second socket");
	if ( bind(fd2, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "second bind");
	if ( connect(fd2, (const struct sockaddr*) &local, locallen) < 0 )
		err(1, "second connect");
	struct sockaddr_in local2;
	socklen_t locallen2 = sizeof(local2);
	if ( getsockname(fd2, (struct sockaddr*) &local2, &locallen2) < 0 )
		err(1, "second getsockname");
	int fd3 = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd3 < 0 )
		err(1, "second socket");
	if ( bind(fd3, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "second bind");
	struct sockaddr_in local3;
	socklen_t locallen3 = sizeof(local3);
	if ( getsockname(fd3, (struct sockaddr*) &local3, &locallen3) < 0 )
		err(1, "second getsockname");
	if ( connect(fd3, (const struct sockaddr*) &local, locallen) < 0 )
		err(1, "second connect");
	char y = 'y';
	if ( send(fd3, &y, sizeof(y), 0) < 0 )
		err(1, "send of y");
	usleep(50000);
	if ( connect(fd, (const struct sockaddr*) &local2, locallen2) < 0 )
		err(1, "first connect");
	char x = 'x';
	if ( send(fd2, &x, sizeof(x), 0) < 0 )
		err(1, "send of x");
	usleep(50000);
	struct pollfd pfd;
	memset(&pfd, 0, sizeof(pfd));
	pfd.fd = fd;
	pfd.events = POLLIN | POLLOUT;
	int num_events = poll(&pfd, 1, 0);
	if ( num_events < 0 )
		err(1, "poll");
	if ( num_events == 0 )
		errx(1, "poll returned 0");
	printf("0");
	if ( pfd.revents & POLLIN )
		printf(" | POLLIN");
	if ( pfd.revents & POLLPRI )
		printf(" | POLLPRI");
	if ( pfd.revents & POLLOUT )
		printf(" | POLLOUT");
#if defined(POLLRDHUP) && POLLRDHUP != POLLHUP
	if ( pfd.revents & POLLRDHUP )
		printf(" | POLLRDHUP");
#endif
	if ( pfd.revents & POLLERR )
		printf(" | POLLERR");
	if ( pfd.revents & POLLHUP )
		printf(" | POLLHUP");
#if POLLRDNORM != POLLIN
	if ( pfd.revents & POLLRDNORM )
		printf(" | POLLRDNORM");
#endif
#if POLLRDBAND != POLLPRI
	if ( pfd.revents & POLLRDBAND )
		printf(" | POLLRDBAND");
#endif
#if POLLWRNORM != POLLOUT
	if ( pfd.revents & POLLWRNORM )
		printf(" | POLLWRNORM");
#endif
#if POLLWRBAND != POLLOUT
	if ( pfd.revents & POLLWRBAND )
		printf(" | POLLWRBAND");
#endif
	putchar('\n');
	return 0;
}
