#include <inttypes.h>
#ifdef strtoumax
#undef strtoumax
#endif
uintmax_t (*foo)(const char *restrict, char **restrict, int) = strtoumax;
int main(void) { return 0; }
