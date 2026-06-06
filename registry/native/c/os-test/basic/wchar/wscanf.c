/* Test whether a basic wscanf invocation works. */

#include <wchar.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	close(0);
	close(1);
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	if ( fputws(L"hello world 42", stdout) == EOF )
		err(1, "fputws");
	if ( fflush(stdout) == EOF )
		err(1, "fflush");
	close(1);
	wchar_t world[6];
	int value;
	int ret = wscanf(L"hello %5ls %d", world, &value);
	if ( ret < 0 )
		err(1, "swscanf");
	if ( ret != 2 )
		errx(1, "swscanf did not return 2");
	if ( wcscmp(world, L"world") != 0 )
		errx(1, "swscanf gave '%ls' instead of '%ls'", world, L"world");
	if ( value != 42 )
		errx(1, "swscanf gave %d' instead of %d", value, 42);
	return 0;
}
