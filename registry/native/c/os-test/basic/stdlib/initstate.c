/*[XSI]*/
/* Test whether a basic initstate invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char state[256];
	memset(state, 0, sizeof(state));
	char* old_state = initstate(1337, state, 256);
	if ( !old_state )
		errx(1, "initstate returned NULL");
	return 0;
}
