/*[XSI]*/
/* Test whether a basic setstate invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	char state[256] = {1};
	char* old_state = initstate(42, state, sizeof(state));
	if ( !old_state )
		errx(1, "initstate returned NULL");
	char new_state[256] = {2};
	old_state = setstate(new_state);
	if ( !old_state )
		errx(1, "setstate returned NULL");
	if ( old_state != state )
		errx(1, "setstate did not return the old state pointer");
	return 0;
}
