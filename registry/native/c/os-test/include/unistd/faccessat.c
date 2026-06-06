#include <unistd.h>
#ifdef faccessat
#undef faccessat
#endif
int (*foo)(int, const char *, int, int) = faccessat;
int main(void) { return 0; }
