/* Test whether a basic cfgetispeed invocation works. */

#include <termios.h>

#include "../basic.h"

int main(void)
{
	struct termios tio = {0};
	if ( cfsetispeed(&tio, B9600) < 0 )
		err(1, "cfsetispeed");
	if ( cfgetispeed(&tio) != B9600 )
		err(1, "cfgetispeed did not return B9600");
	return 0;
}
