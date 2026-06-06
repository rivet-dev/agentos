/* Test whether a basic getopt invocation works. */

#include <string.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	int argc = 4;
	char* argv[] = { "getopt", "-xfoo", "--", "-y", NULL };
	if ( getopt(argc, argv, "x:y") != 'x' )
		err(1, "first getopt did not return x");
	if ( !optarg )
		err(1, "first getopt did set optarg");
	if ( strcmp(optarg, "foo") != 0 )
		err(1, "first getopt set optarg to the wrong value");
	if ( getopt(argc, argv, "x:y") != -1 )
		err(1, "second getopt did not return x");
	if ( optind != 3 )
		err(1, "second getopt did set optind to 3");
	return 0;
}
