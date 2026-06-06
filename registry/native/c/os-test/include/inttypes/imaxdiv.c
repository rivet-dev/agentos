#include <inttypes.h>
#ifdef imaxdiv
#undef imaxdiv
#endif
imaxdiv_t (*foo)(intmax_t, intmax_t) = imaxdiv;
int main(void) { return 0; }
