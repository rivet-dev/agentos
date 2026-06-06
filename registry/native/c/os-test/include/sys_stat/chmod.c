#include <sys/stat.h>
#ifdef chmod
#undef chmod
#endif
int (*foo)(const char *, mode_t) = chmod;
int main(void) { return 0; }
