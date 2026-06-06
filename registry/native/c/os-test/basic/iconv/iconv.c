/* Test whether a basic iconv invocation works. */

#include <iconv.h>

#include "../basic.h"

int main(void)
{
	// de facto: Unfortunately POSIX fails to standardize names for the
	// available encodings, and fails to provide a way to find the available
	// names except iconv -l whose format is unspecified. That means that
	// conforming applications have no way to actually invoke this interface.
	// However, the basic names like UTF-8 and UTF-16LE are available everywhere
	// and I would argue that those basic names should be standardized. In this
	// test, we rely on the names. If any new implementations fail to use these
	// names, well yes that's allowed, but no, they should align with tradition.
	iconv_t conv = iconv_open("UTF-8", "UTF-16LE");
	if ( conv == (iconv_t) -1 )
		err(1, "iconv_open");
	char utf16le[] = {0x66, 0x00, 0xf8, 0x00, 0xf8, 0x00, 0x20,
	                  0x00, 0x62, 0x00, 0xe1, 0x00, 0x72, 0x00};
	const char expected[] = u8"føø bár";
	size_t expected_len = sizeof(expected) - 1;
	char* input = utf16le;
	size_t input_left = 5;
	char utf8[16];
	char* output = utf8;
	size_t output_left = sizeof(utf8) - 1;
	// Try converting a partial sequence.
	size_t result = iconv(conv, &input, &input_left, &output, &output_left);
	if ( result == (size_t) -1 )
	{
		if ( errno != EINVAL )
			err(1, "first iconv");
	}
	else
		errx(1, "iconv was supposed to fail on a partial sequence");
	// The partial character might be left, or might be part of the shift state.
	if ( 2 <= input_left )
		errx(1, "more than one byte was left");
	// Try converting the rest.
	input_left = utf16le + sizeof(utf16le) - input;
	result = iconv(conv, &input, &input_left, &output, &output_left);
	if ( result == (size_t) -1 )
		err(1, "second iconv");
	if ( input_left != 0 )
		errx(1, "no input was supposed to be left");
	*output = '\0';
	size_t output_len = output - utf8;
	if ( output_len != expected_len )
		err(1, "output was %zu bytes instead of %zu", output_len, expected_len);
	if ( memcmp(utf8, expected, expected_len) != 0 )
		errx(1, "wrong output: %s vs %s", utf8, expected);
	// Try restoring the initial shift state. This is a no-op for UTF-16 but
	// let's see if it happens to crash on a null pointer dereference. POSIX
	// says a shift state reset happens if inbuf is NULL, or if it points to a
	// NULL string.
	input_left = 0;
	result = iconv(conv, NULL, &input_left, &output, &output_left);
	if ( result == (size_t) -1 )
		err(1, "third iconv");
	input = NULL;
	input_left = 0;
	result = iconv(conv, &input, &input_left, &output, &output_left);
	if ( result == (size_t) -1 )
		err(1, "fourth iconv");
	return 0;
}
