/*[OB]*/
/* Test whether a basic inet_ntoa invocation works. */

#include <arpa/inet.h>

#include "../basic.h"

int main(void)
{
	struct in_addr input = { .s_addr = htonl(0x01020304) };
	char* output = inet_ntoa(input);
	const char* expected = "1.2.3.4";
	if ( strcmp(output, expected) != 0 )
		err(1, "inet_ntoa() = '%s' not '%s'", output, expected);
	return 0;
}
