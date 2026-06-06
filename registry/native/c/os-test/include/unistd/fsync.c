/*[FSC]*/
#include <unistd.h>
#ifdef fsync
#undef fsync
#endif
int (*foo)(int) = fsync;
int main(void) { return 0; }
