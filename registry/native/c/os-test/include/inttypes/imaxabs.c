#include <inttypes.h>
#ifdef imaxabs
#undef imaxabs
#endif
intmax_t (*foo)(intmax_t) = imaxabs;
int main(void) { return 0; }
