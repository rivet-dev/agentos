/*[SPN]*/
/* Test whether a basic posix_spawn_file_actions_destroy invocation works. */

#include <spawn.h>

#include "../basic.h"

int main(void)
{
	posix_spawn_file_actions_t actions;
	if ( (errno = posix_spawn_file_actions_init(&actions)) )
		err(1, "posix_spawn_file_actions_init");
	if ( (errno = posix_spawn_file_actions_destroy(&actions)) )
		err(1, "posix_spawn_file_actions_destroy");
	return 0;
}
