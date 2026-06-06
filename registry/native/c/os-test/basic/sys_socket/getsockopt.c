/* Test whether a basic getsockopt invocation works. */

#include <sys/socket.h>

#include "../basic.h"

int main(void)
{
	int listen_fd = socket(AF_INET, SOCK_STREAM, 0);
	if ( listen_fd < 0 )
		err(1, "socket");
	int value = -1;
	socklen_t length = sizeof(value);
	if ( getsockopt(listen_fd, SOL_SOCKET, SO_REUSEADDR, &value, &length) < 0 )
		err(1, "getsockopt");
	if ( length != sizeof(value) )
		err(1, "getsockopt returned odd length");
	if ( value != 0 )
		errx(1, "getsockopt gave %d instead of 0", value);
	return 0;
}
