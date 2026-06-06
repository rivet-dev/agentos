/* Test whether a basic setsockopt invocation works. */

#include <sys/socket.h>

#include "../basic.h"

int main(void)
{
	int listen_fd = socket(AF_INET, SOCK_STREAM, 0);
	if ( listen_fd < 0 )
		err(1, "socket");
	int value = 1;
	if ( setsockopt(listen_fd, SOL_SOCKET, SO_REUSEADDR, &value,
	                sizeof(value)) < 0 )
		err(1, "setsockopt");
	value = -1;
	socklen_t length = sizeof(value);
	if ( getsockopt(listen_fd, SOL_SOCKET, SO_REUSEADDR, &value, &length) < 0 )
		err(1, "getsockopt");
	if ( length != sizeof(value) )
		err(1, "getsockopt returned odd length");
	if ( value == 0 )
		errx(1, "socket option was not set");
	return 0;
}
