#include <inttypes.h>
#ifdef strtoimax
#undef strtoimax
#endif
intmax_t (*foo)(const char *restrict, char **restrict, int) = strtoimax;
int main(void) { return 0; }
