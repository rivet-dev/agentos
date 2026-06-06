/*[OB]*/
/* Test whether a basic inet_addr invocation works. */

#include <arpa/inet.h>

#include "../basic.h"

int main(void)
{
	in_addr_t value;
	in_addr_t expected;
	const char* str;
	value = inet_addr(str = "1.2.3.4");
	if ( value != (expected = htonl(0x01020304)) )
		err(1, "inet_addr(\"%s\") = 0x%08x, not 0x%08x", str, value, expected);
	value = inet_addr(str = "0xA.0XBC.345");
	if ( value != (expected = htonl(0x0abc0159)) )
		err(1, "inet_addr(\"%s\") = 0x%08x, not 0x%08x", str, value, expected);
	value = inet_addr(str = "0x0.007777");
	if ( value != (expected = htonl(0x00000fff)) )
		err(1, "inet_addr(\"%s\") = 0x%08x, not 0x%08x", str, value, expected);
	value = inet_addr(str = "123456789");
	if ( value != (expected = htonl(0x075bcd15)) )
		err(1, "inet_addr(\"%s\") = 0x%08x, not 0x%08x", str, value, expected);
	value = inet_addr(str = " 1.2.3.4");
	if ( value != (expected = (in_addr_t) -1) )
		err(1, "inet_addr(\"%s\") = 0x%08x, not 0x%08x", str, value, expected);
	return 0;
}
