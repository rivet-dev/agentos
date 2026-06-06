/* Test whether a basic _Fork invocation works. */

#include <unistd.h>

#include "../basic.h"

int main(void)
{
	pid_t pid = _Fork();
	if ( pid < 0 )
		err(1, "_Fork");
	return pid ? 0 : 1;
}
