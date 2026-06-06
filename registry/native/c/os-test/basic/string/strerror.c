/* Test whether a basic strerror invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	if ( !strerror(EILSEQ) )
		errx(1, "strerror returned NULL");
	return 0;
}
