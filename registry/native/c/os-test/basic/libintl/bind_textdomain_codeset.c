/* Test whether a basic bind_textdomain_codeset invocation works. */

#include <locale.h>
#include <libintl.h>

#include "../basic.h"

int main(void)
{
	// Try getting the default codeset.
	errno = 0;
	char* codeset = bind_textdomain_codeset("os-test", NULL);
	if ( !codeset && errno )
		err(1, "bind_textdomain_codeset");
	// POSIX problem: The description of bind_textdomain_codeset says that the
	// function returns the default codeset in this case and makes no mention of
	// a null return, but the return value section mentions it returns NULL in
	// this case. GNU gettext returns NULL in this case, which presuambly is the
	// intended behavior as the GNU implementation got standardized. This needs
	// to be fixed in the standard.
#if 0
	if ( !codeset )
		err(1, "bind_textdomain_codeset");
	if ( !codeset[0] )
		errx(1, "default codeset was empty");
#endif
	return 0;
}
