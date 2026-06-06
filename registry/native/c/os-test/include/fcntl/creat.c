#include <fcntl.h>
#ifdef creat
#undef creat
#endif
int (*foo)(const char *, mode_t) = creat;
int main(void) { return 0; }
