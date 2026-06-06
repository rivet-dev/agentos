/*[SIO]*/
#include <unistd.h>
#ifdef fdatasync
#undef fdatasync
#endif
int (*foo)(int) = fdatasync;
int main(void) { return 0; }
