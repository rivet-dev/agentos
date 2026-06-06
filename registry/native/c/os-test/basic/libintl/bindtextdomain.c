/* Test whether a basic bindtextdomain invocation works. */

#include <errno.h>
#include <libintl.h>

#include "../basic.h"

int main(void)
{
	// Get the default textdomain binding.
	char* path = bindtextdomain("os-test", NULL);
	if ( !path )
		err(1, "bindtextdomain");
	if ( !path[0] )
		errx(1, "bindtextdomain did not have default locale path");
	// Test bindtextdomain on NULL returning NULL without changing errno.
	errno = 0;
	if ( bindtextdomain(NULL, NULL) )
		errx(1, "bindtextdomain(NULL, NULL) != NULL");
	else if ( errno )
		err(1, "bindtextdomain(NULL, NULL)");
	// Test bindtextdomain on "" returning NULL without changing errno.
	if ( bindtextdomain("", NULL) )
		errx(1, "bindtextdomain(\"\", NULL) != NULL");
	else if ( errno )
		err(1, "bindtextdomain(\"\", NULL)");
	return 0;
}
