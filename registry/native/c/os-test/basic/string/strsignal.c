/* Test whether a basic strsignal invocation works. */

#include <signal.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	if ( !strsignal(SIGABRT) )
		errx(1, "strsignal returned NULL");
	return 0;
}
