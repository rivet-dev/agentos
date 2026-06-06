/*[XSI]*/
/* Test whether a basic ftok invocation works. */

#include <sys/ipc.h>

#include "../basic.h"

int main(void)
{
	key_t key = ftok("sys_ipc/ftok", 'f');
	if ( key == (key_t) -1 )
		err(1, "ftok");
	return 0;
}
