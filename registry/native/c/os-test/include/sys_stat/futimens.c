#include <sys/stat.h>
#ifdef futimens
#undef futimens
#endif
int (*foo)(int, const struct timespec [2]) = futimens;
int main(void) { return 0; }
