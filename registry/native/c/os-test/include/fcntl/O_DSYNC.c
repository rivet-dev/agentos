/*[SIO]*/
#include <fcntl.h>
#ifndef O_DSYNC
#error "O_DSYNC is not defined"
#endif
int main(void) { return 0; }
