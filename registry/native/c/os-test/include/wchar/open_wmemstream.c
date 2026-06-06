#include <wchar.h>
#ifdef open_wmemstream
#undef open_wmemstream
#endif
FILE *(*foo)(wchar_t **, size_t *) = open_wmemstream;
int main(void) { return 0; }
