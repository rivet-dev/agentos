/* Test whether a basic cfgetospeed invocation works. */

#include <termios.h>

#include "../basic.h"

int main(void)
{
	struct termios tio = {0};
	if ( cfsetospeed(&tio, B9600) < 0 )
		err(1, "cfsetospeed");
	if ( cfgetospeed(&tio) != B9600 )
		err(1, "cfgetospeed did not return B9600");
	return 0;
}
