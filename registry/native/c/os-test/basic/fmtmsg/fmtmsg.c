/*[XSI]*/
/* Test whether a basic fmtmsg invocation works. */

#include <fmtmsg.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	// Avoid any existing environment variable from interfering with the test.
	unsetenv("MSGVERB");
	// Capture the message written to stderr using a pipe.
	int real_stderr = dup(2);
	close(0);
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	dup2(fds[1], 2);
	// Run the test.
	int ret = fmtmsg(MM_SOFT | MM_APPL | MM_PRINT | MM_RECOVER,
	                 "os-test:fmtmsg", MM_INFO, "os-test is running",
	                 "Run os-test again", "os-test:fmtmsg:1");
	// Restore stderr and close the pipe.
	dup2(real_stderr, 2);
	close(fds[1]);
	if ( ret != MM_OK )
	{
		if ( ret == MM_NOTOK )
			errx(1, "fmtmsg failed entirely");
		else if ( ret == MM_NOMSG )
			errx(1, "fmtmsg failed to write to stderr");
		else if ( ret == MM_NOCON )
			errx(1, "fmtmsg failed to write to console");
		else
			errx(1, "fmtmsg failed weirdly");
	}
	// Read what message was written by fmtmsg.
	char output[512];
	size_t amount = fread(output, 1, sizeof(output) - 1, stdin);
	if ( ferror(stdin) )
		err(1, "fread");
	output[amount] = '\0';
	// Although POSIX technically doesn't fully specify the output format, it is
	// de-facto one of these three formats, so let's simply require using one of
	// those formats. If a fourth kind of system is discovered, we can loosen up
	// this test, and instead test that all the required information is
	// contained within the message. However, it's probably better if systems
	// stick to the established convention.
	const char* expected1 = "os-test:fmtmsg: INFO: os-test is running\n"
	                        "TO FIX: Run os-test again os-test:fmtmsg:1\n";
	const char* expected2 = "os-test:fmtmsg: INFO: os-test is running\n"
	                        "TO FIX: Run os-test again  os-test:fmtmsg:1\n";
	const char* expected3 = "os-test:fmtmsg: INFO: os-test is running\n"
	                        "              "
	                        "TO FIX: Run os-test again  os-test:fmtmsg:1\n";
	if ( strcmp(output, expected1) != 0 &&
	     strcmp(output, expected2) != 0 &&
	     strcmp(output, expected3) != 0 )
		errx(1, "wrong fmtmsg output\n%s%s", output, expected3);
	return 0;
}
