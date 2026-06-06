/* Test whether a basic socketpair invocation works. */

#include <sys/socket.h>

#include "../basic.h"

int main(void)
{
	int fds[2];
	if ( socketpair(AF_UNIX, SOCK_STREAM, 0, fds) < 0 )
		err(1, "socketpair");
	return 0;
}
